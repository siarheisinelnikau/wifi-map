"""Build/test with installed SDK and cached Maven artifacts; no daemon, device or network."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[1]
NS = {'m': 'http://maven.apache.org/POM/4.0.0'}
parser = argparse.ArgumentParser()
parser.add_argument('--java-home', type=Path, required=True)
parser.add_argument('--sdk', type=Path, required=True)
parser.add_argument('--cache', type=Path, default=Path.home()/'.gradle/caches/modules-2/files-2.1')
parser.add_argument('--test-only', action='store_true')
parser.add_argument('--d8-jar', type=Path, help='D8/R8 8.6.17 or newer; defaults to installed SDK build tools 36')
args = parser.parse_args()
OUT = ROOT/'build/offline'
if OUT.exists():
    if not OUT.resolve().is_relative_to(ROOT.resolve()/'build'):
        raise RuntimeError('Offline output must remain inside this project build directory')
    shutil.rmtree(OUT)
OUT.mkdir(parents=True, exist_ok=True)
JAVA = args.java_home/'bin/java.exe'

def run(command):
    subprocess.run([str(s) for s in command], cwd=ROOT, check=True)

def java(classpath, main, *arguments):
    run([JAVA, '-Xmx1536m', '-XX:ActiveProcessorCount=2', '-cp', os.pathsep.join(map(str, classpath)), main, *arguments])

def cached(group, artifact, version, extension):
    folder = args.cache/group/artifact/version
    files = list(folder.glob(f'*/{artifact}-{version}.{extension}'))
    if not files:
        raise RuntimeError(f'Missing cached artifact: {group}:{artifact}:{version} ({extension}); no download attempted')
    return files[0]

def version_key(version):
    return tuple(int(p) if p.isdigit() else p for p in re.split(r'[.-]', version))

RAW_JAR_DEPENDENCIES = {
    ('org.jetbrains', 'annotations'): [],
    ('org.jetbrains.kotlin', 'kotlin-reflect'): [('org.jetbrains.kotlin', 'kotlin-stdlib', '2.1.21')],
    ('org.jetbrains.kotlin', 'kotlin-script-runtime'): [],
    ('org.jetbrains.kotlin', 'kotlin-daemon-embeddable'): [],
    ('org.jetbrains.intellij.deps', 'trove4j'): [],
    ('org.jetbrains.kotlinx', 'kotlinx-coroutines-core-jvm'): [('org.jetbrains.kotlin', 'kotlin-stdlib', '2.1.21')],
    ('junit', 'junit'): [('org.hamcrest', 'hamcrest-core', '1.3')],
    ('org.hamcrest', 'hamcrest-core'): [],
    ('androidx.annotation', 'annotation-jvm'): [('org.jetbrains.kotlin', 'kotlin-stdlib', '2.1.21')],
    ('androidx.arch.core', 'core-common'): [('androidx.annotation', 'annotation', '1.9.1')],
    ('androidx.collection', 'collection'): [('androidx.annotation', 'annotation', '1.9.1')],
    ('androidx.lifecycle', 'lifecycle-common'): [('androidx.annotation', 'annotation', '1.9.1'), ('org.jetbrains.kotlin', 'kotlin-stdlib', '2.1.21')],
}

# Bounded cache-only fallback for AndroidX archives whose POM is not installed.
# These are dependency edges, not replacement binaries or generated Maven files.
RAW_AAR_DEPENDENCIES = {
    ('androidx.core', 'core-viewtree', '1.0.0'): [],
    ('androidx.lifecycle', 'lifecycle-runtime', '2.6.2'): [('androidx.lifecycle', 'lifecycle-common', '2.6.2'), ('androidx.arch.core', 'core-common', '2.2.0')],
    ('androidx.lifecycle', 'lifecycle-viewmodel', '2.6.2'): [],
    ('androidx.lifecycle', 'lifecycle-viewmodel-savedstate', '2.6.2'): [('androidx.lifecycle', 'lifecycle-livedata-core', '2.6.2'), ('androidx.lifecycle', 'lifecycle-runtime-ktx', '2.6.2'), ('androidx.lifecycle', 'lifecycle-viewmodel-ktx', '2.6.2'), ('androidx.savedstate', 'savedstate', '1.2.1')],
    ('androidx.lifecycle', 'lifecycle-livedata-core', '2.6.2'): [('androidx.arch.core', 'core-runtime', '2.2.0'), ('androidx.lifecycle', 'lifecycle-common', '2.6.2')],
    ('androidx.arch.core', 'core-runtime', '2.2.0'): [('androidx.arch.core', 'core-common', '2.2.0')],
    ('androidx.savedstate', 'savedstate', '1.2.1'): [('androidx.arch.core', 'core-common', '2.2.0'), ('androidx.lifecycle', 'lifecycle-common', '2.6.2')],
    ('androidx.annotation', 'annotation-experimental', '1.4.1'): [('org.jetbrains.kotlin', 'kotlin-stdlib', '2.1.21')],
    ('androidx.interpolator', 'interpolator', '1.0.0'): [],
    ('androidx.tracing', 'tracing', '1.0.0'): [],
    ('androidx.versionedparcelable', 'versionedparcelable', '1.1.1'): [('androidx.collection', 'collection', '1.1.0')],
}

def metadata(packaging, edges):
    project = ET.Element('{'+NS['m']+'}project')
    ET.SubElement(project, '{'+NS['m']+'}packaging').text = packaging
    dependencies = ET.SubElement(project, '{'+NS['m']+'}dependencies')
    for g,a,v in edges:
        dep = ET.SubElement(dependencies, '{'+NS['m']+'}dependency')
        for key,value in [('groupId',g),('artifactId',a),('version',v)]:
            ET.SubElement(dep, '{'+NS['m']+'}'+key).text=value
    return project

def read_pom(group, artifact, version):
    if (group, artifact) == ('androidx.annotation','annotation') and version == '1.9.1':
        return ET.fromstring('<project xmlns="'+NS['m']+'"><packaging>pom</packaging><dependencies><dependency><groupId>androidx.annotation</groupId><artifactId>annotation-jvm</artifactId><version>1.9.1</version></dependency></dependencies></project>')
    try:
        return ET.parse(cached(group, artifact, version, 'pom')).getroot()
    except RuntimeError:
        if (group, artifact, version) in RAW_AAR_DEPENDENCIES:
            cached(group, artifact, version, 'aar')
            return metadata('aar', RAW_AAR_DEPENDENCIES[group, artifact, version])
        source = cached(group, artifact, version, 'jar')
        with zipfile.ZipFile(source) as archive:
            embedded = f'META-INF/maven/{group}/{artifact}/pom.xml'
            if embedded in archive.namelist():
                return ET.fromstring(archive.read(embedded))
        if (group, artifact) not in RAW_JAR_DEPENDENCIES:
            raise RuntimeError(f'Missing dependency metadata: {group}:{artifact}:{version}')
        # Explicit leaves/tool artifacts whose cached JARs omit Maven metadata.
        return metadata('jar', RAW_JAR_DEPENDENCIES[group, artifact])

def properties(node, group, artifact, version):
    result = {'project.version': version, 'project.groupId': group, 'project.artifactId': artifact}
    parent = node.find('m:parent', NS)
    if parent is not None:
        pg, pa, pv = (parent.findtext('m:'+key, namespaces=NS) for key in ('groupId', 'artifactId', 'version'))
        result['project.parent.version'] = pv
        try:
            result.update(properties(ET.parse(cached(pg, pa, pv, 'pom')).getroot(), pg, pa, pv))
        except RuntimeError:
            pass
        result.update({'project.version': version, 'project.groupId': group, 'project.artifactId': artifact})
    local = node.find('m:properties', NS)
    if local is not None:
        result.update({p.tag.split('}')[-1]: p.text for p in local})
    return result

def resolve(roots, overrides=None):
    overrides = overrides or {}
    selected = {(g, a): v for g, a, v in roots}
    selected.update(overrides)
    for _ in range(32):
        changed = False
        for (group, artifact), version in list(selected.items()):
            pom = read_pom(group, artifact, version)
            props = properties(pom, group, artifact, version)
            managed = {(d.findtext('m:groupId', namespaces=NS), d.findtext('m:artifactId', namespaces=NS)): d.findtext('m:version', namespaces=NS) for d in pom.findall('m:dependencyManagement/m:dependencies/m:dependency', NS)}
            for dep in pom.findall('m:dependencies/m:dependency', NS):
                if dep.findtext('m:scope', namespaces=NS) not in (None, 'compile', 'runtime') or dep.findtext('m:optional', namespaces=NS) == 'true':
                    continue
                key = (dep.findtext('m:groupId', namespaces=NS), dep.findtext('m:artifactId', namespaces=NS))
                desired = overrides.get(key) or dep.findtext('m:version', namespaces=NS) or managed.get(key)
                if desired is None:
                    if key in selected:
                        continue
                    raise RuntimeError(f'Unresolved dependency version: {key} from {group}:{artifact}:{version}')
                for _ in range(4):
                    desired = re.sub(r'\$\{([^}]+)\}', lambda m: props.get(m[1], m[0]), desired)
                desired = desired.strip('[]')
                if '${' in desired or ',' in desired:
                    raise RuntimeError(f'Unsupported unresolved version: {key} {desired}')
                if key not in selected or (key not in overrides and version_key(desired) > version_key(selected[key])):
                    selected[key] = desired
                    changed = True
        if not changed:
            break
    else:
        raise RuntimeError('Dependency resolution did not settle')
    artifacts = []
    for (group, artifact), version in sorted(selected.items()):
        pom = read_pom(group, artifact, version)
        kind = pom.findtext('m:packaging', default='jar', namespaces=NS)
        if kind == 'pom':
            continue
        source = cached(group, artifact, version, kind)
        artifacts.append((group, artifact, version, kind, source))
    return artifacts

def unpack(artifacts, key):
    jars, libraries = [], []
    for group, artifact, version, kind, source in artifacts:
        if kind == 'jar':
            with zipfile.ZipFile(source) as archive:
                if any(n.endswith('.class') for n in archive.namelist()):
                    jars.append(source)
        elif kind == 'aar':
            folder = OUT/'libraries'/f'{group}.{artifact}-{version}'
            folder.mkdir(parents=True, exist_ok=True)
            with zipfile.ZipFile(source) as archive:
                for name in archive.namelist():
                    target = (folder/name).resolve()
                    if not target.is_relative_to(folder.resolve()):
                        raise RuntimeError('Unsafe AAR path')
                archive.extractall(folder)
            if (folder/'classes.jar').exists():
                jars.append(folder/'classes.jar')
            jars.extend((folder/'libs').glob('*.jar'))
            libraries.append(folder)
    records = [{'group':g, 'name':a, 'version':v, 'type':k, 'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for g,a,v,k,p in artifacts]
    (OUT/f'{key}-dependencies.json').write_text(json.dumps(records, indent=2)+'\n', encoding='utf-8')
    return jars, libraries

standard = {('org.jetbrains.kotlin', 'kotlin-stdlib'): '2.1.21', ('org.jetbrains.kotlin', 'kotlin-stdlib-common'): '2.1.21', ('org.jetbrains', 'annotations'):'23.0.0'}
compiler, _ = unpack(resolve([('org.jetbrains.kotlin', 'kotlin-compiler-embeddable', '2.1.21'), ('org.jetbrains', 'annotations', '23.0.0')], standard), 'compiler')
core, _ = unpack(resolve([('com.google.code.gson', 'gson', '2.11.0'), ('org.jetbrains.kotlin', 'kotlin-stdlib', '2.1.21')], standard), 'domain')

def kotlin(source, output, classpath):
    output.mkdir(parents=True, exist_ok=True)
    sources = sorted(source.rglob('*.kt'))
    if not sources:
        raise RuntimeError(f'No Kotlin sources: {source}')
    arguments = ['-no-stdlib', '-no-reflect', '-jvm-target', '17', '-module-name', 'wifimap_'+output.name, '-classpath', os.pathsep.join(map(str, classpath)), '-d', str(output), *map(str, sources)]
    response = OUT/(output.name+'.args')
    response.write_text('\n'.join('"'+str(arg).replace('\\', '/').replace('"', '\\"')+'"' for arg in arguments), encoding='utf-8')
    java(compiler, 'org.jetbrains.kotlin.cli.jvm.K2JVMCompiler', '@'+str(response))

classes = OUT/'domain-classes'
kotlin(ROOT/'domain/src/main/kotlin', classes, core)
test_source = ROOT/'domain/src/test/kotlin'
if test_source.exists():
    test_jars, _ = unpack(resolve([('junit', 'junit', '4.13.2')]), 'tests')
    tests = OUT/'test-classes'
    kotlin(test_source, tests, [classes, *core, *test_jars])
    test_names = [p.read_text(encoding='utf-8').split('package ',1)[1].splitlines()[0].strip()+'.'+p.stem for p in sorted(test_source.rglob('*Test.kt'))]
    run([JAVA, '-Xmx768m', '-XX:ActiveProcessorCount=2', '-Dinterop.output='+str(ROOT/'domain/build/interop/android-roundtrip.wifimap'), '-cp', os.pathsep.join(map(str,[tests,classes,ROOT.parent/'shared/fixtures',*core,*test_jars])), 'org.junit.runner.JUnitCore', *test_names])
if args.test_only:
    print('Domain compiler and tests completed without Gradle, sockets, SDK changes or device access.')
    raise SystemExit(0)

subprocess.run([os.sys.executable,str(ROOT/'tools/prepare_assets.py')],cwd=ROOT,check=True)
runtime_roots = [('androidx.activity','activity','1.10.1'),('androidx.lifecycle','lifecycle-viewmodel-ktx','2.6.2'),('androidx.lifecycle','lifecycle-runtime-ktx','2.6.2'),('org.jetbrains.kotlinx','kotlinx-coroutines-android','1.10.2'),('com.google.code.gson','gson','2.11.0'),('org.jetbrains.kotlin','kotlin-stdlib','2.1.21'),('androidx.annotation','annotation','1.9.1')]
runtime_pins = dict(standard)
runtime_pins[('androidx.annotation','annotation')] = '1.9.1'
runtime_pins[('androidx.annotation','annotation-jvm')] = '1.9.1'
runtime_pins[('org.jetbrains.kotlinx','kotlinx-coroutines-core')] = '1.10.2'
runtime_pins[('org.jetbrains.kotlinx','kotlinx-coroutines-core-jvm')] = '1.10.2'
for artifact in ['lifecycle-common','lifecycle-runtime','lifecycle-viewmodel','lifecycle-viewmodel-savedstate']:
    runtime_pins[('androidx.lifecycle', artifact)] = '2.6.2'
runtime_pins.update({('androidx.annotation','annotation-experimental'):'1.4.1', ('androidx.collection','collection'):'1.1.0', ('androidx.concurrent','concurrent-futures'):'1.2.0', ('androidx.profileinstaller','profileinstaller'):'1.4.1', ('androidx.startup','startup-runtime'):'1.2.0'})
runtime_artifacts = resolve(runtime_roots,runtime_pins)
runtime_jars, libraries = unpack(runtime_artifacts,'runtime')
sdk = args.sdk.resolve()
android_jar = sdk/'platforms/android-35/android.jar'
build_tools = sdk/'build-tools/35.0.0'
if not android_jar.exists() or not (build_tools/'aapt2.exe').exists():
    raise SystemExit('Android SDK platform 35 and build tools 35.0.0 are required; none were installed.')
d8 = args.d8_jar or sdk/'build-tools/36.0.0/lib/d8.jar'
if not d8.exists():
    raise SystemExit('A Kotlin 2.1-compatible D8 is required: pass --d8-jar or use installed SDK build tools 36. No SDK was installed.')
d8_version = subprocess.check_output([str(JAVA), '-Xmx256m', '-XX:ActiveProcessorCount=2', '-cp', str(d8), 'com.android.tools.r8.D8', '--version'], text=True)
match = re.search(r'D8 (\d+)\.(\d+)\.(\d+)', d8_version)
if match is None or tuple(map(int, match.groups())) < (8, 6, 17):
    raise SystemExit('D8 8.6.17 or newer is required for Kotlin 2.1 metadata.')
print(d8_version.strip())

# Keep the app's legal notices with both debug and eventual release builds.
notices=['WifiMap Android dependency notices','', 'Runtime dependencies:']
for g,a,v,kind,file in runtime_artifacts:
    notices.append(f'{g}:{a}:{v}')
    try:
        pom=read_pom(g,a,v)
        for license_node in pom.findall('m:licenses/m:license',NS):
            notices.append('  '+license_node.findtext('m:name',default='',namespaces=NS)+' '+license_node.findtext('m:url',default='',namespaces=NS))
    except Exception:
        pass
legal={}
for file in runtime_jars:
    with zipfile.ZipFile(file) as archive:
        for name in archive.namelist():
            if re.search(r'(^|/)(LICENSE[^/]*|NOTICE[^/]*|COPYING[^/]*)$',name,re.I) and not name.endswith('/'):
                content=archive.read(name).decode('utf-8',errors='replace').strip()
                if content:
                    legal.setdefault(hashlib.sha256(content.encode()).hexdigest(),([file.name+'/'+name],content))
gradle_license=ROOT/'gradle/wrapper/LICENSE'
apache=gradle_license.read_text(encoding='utf-8').split('Apache License',1)
if len(apache)!=2:
    raise SystemExit('Missing bundled Apache 2.0 license text')
notices.extend(['','AndroidX, Kotlin and Gson are distributed under Apache License 2.0.','Copyright notices supplied by their Maven metadata and archives are retained below.','',gradle_license.read_text(encoding='utf-8')])
for references,content in legal.values():
    notices.extend(['','='*72,', '.join(references),'',content])
(ROOT/'app/src/main/assets/THIRD_PARTY_NOTICES.txt').write_text('\n'.join(notices)+'\n',encoding='utf-8')

manifest = ET.parse(ROOT/'app/src/main/AndroidManifest.xml')
base_package = 'com.siarheisinelnikau.wifimap'
application_id = base_package+'.debug'
ANDROID='{http://schemas.android.com/apk/res/android}'
ET.register_namespace('android','http://schemas.android.com/apk/res/android')
manifest.getroot().set('package',base_package)
application=manifest.getroot().find('application')
application.set(ANDROID+'debuggable','true')
for component in application:
    name=component.get(ANDROID+'name','')
    if name.startswith('.'):
        component.set(ANDROID+'name',base_package+name)
namespaces=set()
resources=[]
for index,folder in enumerate(libraries):
    library_manifest=folder/'AndroidManifest.xml'
    if library_manifest.exists():
        lib_root=ET.parse(library_manifest).getroot()
        namespace=lib_root.get('package')
        if namespace:
            namespaces.add(namespace)
        lib_app=lib_root.find('application')
        if lib_app is not None:
            for component in lib_app:
                raw=ET.tostring(component,encoding='unicode').replace('${applicationId}',application_id)
                node=ET.fromstring(raw)
                if node.get(ANDROID+'name') and not any(c.tag==node.tag and c.get(ANDROID+'name')==node.get(ANDROID+'name') for c in application):
                    application.append(node)
    if (folder/'res').exists() and any((folder/'res').rglob('*.xml')):
        output=OUT/f'library-{index}.zip'
        run([build_tools/'aapt2.exe','compile','--dir',folder/'res','-o',output])
        resources.append(output)
manifest_path=OUT/'AndroidManifest.xml'
manifest.write(manifest_path,encoding='utf-8',xml_declaration=True)
app_resources=OUT/'app-resources.zip'
run([build_tools/'aapt2.exe','compile','--dir',ROOT/'app/src/main/res','-o',app_resources])
generated=OUT/'generated'
generated.mkdir(exist_ok=True)
resource_apk=OUT/'resources.ap_'
command=[build_tools/'aapt2.exe','link','-I',android_jar,'--manifest',manifest_path,'--java',generated,'--custom-package',base_package,'--rename-manifest-package',application_id,'--min-sdk-version','26','--target-sdk-version','35','--version-code','2','--version-name','0.2.0','--auto-add-overlay','-A',ROOT/'app/src/main/assets','-o',resource_apk]
if namespaces:
    command+=['--extra-packages',':'.join(sorted(namespaces))]
for resource in [*resources,app_resources]:
    command+=['-R',resource]
run(command)
config=generated/Path(*base_package.split('.'))/'BuildConfig.java'
config.parent.mkdir(parents=True,exist_ok=True)
config.write_text(f'package {base_package}; public final class BuildConfig {{ public static final boolean DEBUG=true; public static final String APPLICATION_ID="{application_id}"; public static final String VERSION_NAME="0.2.0"; public static final int VERSION_CODE=2; }}',encoding='utf-8')
java_classes=OUT/'java-classes'
java_classes.mkdir(exist_ok=True)
java_response=OUT/'java.args'
java_response.write_text('\n'.join('"'+str(p).replace('\\','/')+'"' for p in generated.rglob('*.java')),encoding='utf-8')
run([args.java_home/'bin/javac.exe','-source','17','-target','17','-classpath',android_jar,'-d',java_classes,'@'+str(java_response)])
app_classes=OUT/'app-classes'
kotlin(ROOT/'app/src/main/kotlin',app_classes,[android_jar,java_classes,classes,*runtime_jars])
application_jar=OUT/'application.jar'
with zipfile.ZipFile(application_jar,'w',zipfile.ZIP_DEFLATED) as archive:
    for folder in [classes,java_classes,app_classes]:
        for file in folder.rglob('*'):
            if file.is_file():
                archive.write(file,file.relative_to(folder).as_posix())
dex=OUT/'dex.zip'
arguments=['--min-api','26','--lib',str(android_jar),'--output',str(dex),str(application_jar),*map(str,runtime_jars)]
java([d8],'com.android.tools.r8.D8',*arguments)
unsigned=OUT/'unsigned.apk'
with zipfile.ZipFile(unsigned,'w',zipfile.ZIP_DEFLATED) as output:
    for source in [resource_apk,dex]:
        with zipfile.ZipFile(source) as archive:
            for name in archive.namelist():
                if not name.endswith('/'):
                    output.writestr(name,archive.read(name))
    seen=set(output.namelist())
    for jar in runtime_jars:
        with zipfile.ZipFile(jar) as archive:
            for name in archive.namelist():
                if name.startswith('META-INF/services/') and not name.endswith('/') and name not in seen:
                    output.writestr(name,archive.read(name));seen.add(name)
aligned=OUT/'aligned.apk'
run([build_tools/'zipalign.exe','-f','4',unsigned,aligned])
keystore=ROOT/'.gradle-user/debug.keystore'
if not keystore.exists():
    keystore.parent.mkdir(exist_ok=True)
    run([args.java_home/'bin/keytool.exe','-genkeypair','-keystore',keystore,'-storepass','android','-keypass','android','-alias','androiddebugkey','-dname','CN=Android Debug,O=Android,C=US','-keyalg','RSA','-keysize','2048','-validity','10000','-noprompt'])
distribution=ROOT/'dist'
distribution.mkdir(exist_ok=True)
apk=distribution/'WifiMap-v0.2.0-debug.apk'
run([JAVA,'-Xmx512m','-jar',build_tools/'lib/apksigner.jar','sign','--ks',keystore,'--ks-key-alias','androiddebugkey','--ks-pass','pass:android','--key-pass','pass:android','--out',apk,aligned])
run([JAVA,'-jar',build_tools/'lib/apksigner.jar','verify','--verbose',apk])
(distribution/(apk.name+'.sha256')).write_text(hashlib.sha256(apk.read_bytes()).hexdigest()+'  '+apk.name+'\n',encoding='ascii')
run([os.sys.executable, ROOT/'tools/verify_apk.py', apk, '--sdk', sdk, '--java-home', args.java_home])
print(f'Prepared {apk.name} ({apk.stat().st_size} bytes). No application, device, emulator or installer was launched.')

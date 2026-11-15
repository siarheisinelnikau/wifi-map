"""Inspect the packaged APK without installing or starting an application."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import re
import struct
import subprocess
import zipfile

parser = argparse.ArgumentParser()
parser.add_argument('apk', type=Path)
parser.add_argument('--sdk', type=Path, required=True)
parser.add_argument('--java-home', type=Path)
args = parser.parse_args()
tool_dirs = sorted((args.sdk/'build-tools').glob('*'), key=lambda p: tuple(int(n) for n in re.findall(r'\d+', p.name)))
tools = next((p for p in reversed(tool_dirs) if (p/'lib/apksigner.jar').exists()), None)
if tools is None:
    raise SystemExit('Installed Android build tools are required for static APK inspection.')
java_home = args.java_home or (Path(os.environ['JAVA_HOME']) if 'JAVA_HOME' in os.environ else None)
java = java_home/'bin'/('java.exe' if os.name == 'nt' else 'java') if java_home else 'java'
aapt = tools/('aapt2.exe' if os.name == 'nt' else 'aapt2')

def output(command):
    return subprocess.check_output(list(map(str, command)), text=True, encoding='utf-8', errors='replace')

badging = output([aapt, 'dump', 'badging', args.apk])
package = re.search(r"package: name='([^']+)' versionCode='([^']+)' versionName='([^']+)'", badging)
assert package and package[1] in ('com.siarheisinelnikau.wifimap', 'com.siarheisinelnikau.wifimap.debug'), 'Wrong package'
assert package.groups()[1:] == ('2', '0.2.0'), 'Wrong version'
assert "minSdkVersion:'26'" in badging and "targetSdkVersion:'35'" in badging, 'Wrong supported SDK range'
assert "launchable-activity: name='com.siarheisinelnikau.wifimap.presentation.MainActivity'" in badging, 'Missing launcher'
permissions = set(re.findall(r"uses-permission: name='([^']+)'", badging))
assert {'android.permission.ACCESS_FINE_LOCATION', 'android.permission.NEARBY_WIFI_DEVICES', 'android.permission.INTERNET', 'android.permission.REQUEST_INSTALL_PACKAGES'} <= permissions
assert 'android.permission.ACCESS_BACKGROUND_LOCATION' not in permissions, 'Unexpected background location permission'
icon = re.search(r"application: label='WifiMap' icon='([^']+)'", badging)
assert icon, 'Missing WifiMap launcher icon'
icon_tree = output([aapt, 'dump', 'xmltree', args.apk, '--file', icon[1]])
assert 'vector' in icon_tree and icon_tree.count('pathData') >= 4, 'Damaged vector icon'

def dex_classes(data):
    assert data[:4] == b'dex\n', 'Invalid DEX'
    strings_count, strings_offset = struct.unpack_from('<II', data, 56)
    types_count, types_offset = struct.unpack_from('<II', data, 64)
    classes_count, classes_offset = struct.unpack_from('<II', data, 96)
    strings = []
    for index in range(strings_count):
        at = struct.unpack_from('<I', data, strings_offset + 4*index)[0]
        while data[at] & 128:
            at += 1
        at += 1
        strings.append(data[at:data.index(b'\0', at)].decode('utf-8', errors='replace'))
    types = [strings[struct.unpack_from('<I', data, types_offset+4*i)[0]] for i in range(types_count)]
    return {types[struct.unpack_from('<I', data, classes_offset+32*i)[0]] for i in range(classes_count)}

with zipfile.ZipFile(args.apk) as archive:
    assert len(archive.namelist()) == len(set(archive.namelist())), 'Duplicate APK entries'
    assert icon[1] in archive.namelist()
    catalog = json.loads(archive.read('assets/catalog.json'))
    assert len(catalog) >= 176 and all(len(row) == 6 and all(isinstance(t, str) and t.strip() for t in row) for row in catalog.values()), 'Incomplete localization'
    assert b'MIT License' in archive.read('assets/LICENSE.txt')
    notices = archive.read('assets/THIRD_PARTY_NOTICES.txt')
    assert b'Apache License' in notices and b'kotlinx-coroutines' in notices, 'Missing dependency notices'
    definitions = set()
    for name in archive.namelist():
        if re.fullmatch(r'classes\d*\.dex', name):
            definitions |= dex_classes(archive.read(name))
    required = {
        'com/siarheisinelnikau/wifimap/presentation/MainActivity',
        'com/siarheisinelnikau/wifimap/presentation/SurveyViewModel',
        'com/siarheisinelnikau/wifimap/platform/AndroidWifiScanner',
        'com/siarheisinelnikau/wifimap/domain/ProjectCodec',
        'androidx/activity/ComponentActivity', 'androidx/lifecycle/ViewModel',
        'androidx/lifecycle/LifecycleRegistry', 'androidx/savedstate/SavedStateRegistry',
        'kotlinx/coroutines/android/AndroidDispatcherFactory', 'com/google/gson/Gson',
    }
    assert {'L'+name+';' for name in required} <= definitions, 'Missing application or runtime classes'

signature = output([java, '-Xmx512m', '-XX:ActiveProcessorCount=2', '-jar', tools/'lib/apksigner.jar', 'verify', '--verbose', args.apk])
assert 'Verified using v2 scheme (APK Signature Scheme v2): true' in signature, 'APK v2 signature did not verify'
print(json.dumps({'artifact':args.apk.name, 'package':package[1], 'version':package[3], 'bytes':args.apk.stat().st_size, 'sha256':hashlib.sha256(args.apk.read_bytes()).hexdigest(), 'defined_classes':len(definitions), 'translations':len(catalog), 'signature':'APK v2 verified', 'scope':'Static archive, resources, manifest, DEX and signature checks. No application, emulator, device, scan or installer was started.'}, indent=2))

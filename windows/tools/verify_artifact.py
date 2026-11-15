"""Read PE resources/imports without executing the application or using a GUI."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import zlib

parser = argparse.ArgumentParser()
parser.add_argument('exe', type=Path)
parser.add_argument('--report', type=Path)
parser.add_argument('--icon-output', type=Path)
args = parser.parse_args()
data = args.exe.read_bytes()
u16 = lambda at: struct.unpack_from('<H',data,at)[0]
u32 = lambda at: struct.unpack_from('<I',data,at)[0]
assert data[:2] == b'MZ', 'Missing DOS header'
pe = u32(0x3c)
assert data[pe:pe+4] == b'PE\0\0' and u16(pe+4) == 0x8664, 'Not Windows x64 PE'
optional = pe+24
assert u16(optional) == 0x20b and u16(optional+68) == 2, 'Not a PE32+ Windows GUI executable'
section_table = optional+u16(pe+20)
sections = [struct.unpack_from('<8sIIII',data,section_table+i*40) for i in range(u16(pe+6))]
def offset(rva):
    for _,virtual_size,address,raw_size,raw in sections:
        if address <= rva < address+max(virtual_size,raw_size):
            return raw+rva-address
    raise ValueError(f'Unmapped RVA {rva:x}')

root = offset(u32(optional+112+2*8))
def entries(directory):
    count = u16(directory+12)+u16(directory+14)
    return [(u32(directory+16+i*8),u32(directory+20+i*8)) for i in range(count)]
def resource_ids(type_id):
    type_entry = next(value for key,value in entries(root) if key == type_id)
    return entries(root+(type_entry&0x7fffffff))
def leaf(value):
    if value&0x80000000:
        return leaf(entries(root+(value&0x7fffffff))[0][1])
    at = root+value
    start = offset(u32(at))
    return data[start:start+u32(at+4)]

icons = {key:leaf(value) for key,value in resource_ids(3)}
groups = [leaf(value) for _,value in resource_ids(14)]
sizes = []
chosen = None
for group in groups:
    assert struct.unpack_from('<HH',group) == (0,1), 'Invalid icon group'
    for i in range(struct.unpack_from('<H',group,4)[0]):
        width,height,_,_,_,_,length,key = struct.unpack_from('<BBBBHHIH',group,6+i*14)
        image = icons[key]
        assert len(image) == length and image[:8] == b'\x89PNG\r\n\x1a\n', 'Missing packaged PNG icon'
        actual = struct.unpack_from('>II',image,16)
        assert actual == (width or 256,height or 256), 'Icon extent mismatch'
        at = 8
        packed = bytearray()
        while at < len(image):
            n = struct.unpack_from('>I',image,at)[0]
            if image[at+4:at+8] == b'IDAT':
                packed.extend(image[at+8:at+8+n])
            at += n+12
        raw = zlib.decompress(packed)
        stride = actual[0]*4+1
        assert all(raw[y*stride] == 0 for y in range(actual[1])), 'Unexpected icon PNG filter'
        alpha = [raw[y*stride+1+x*4+3] for y in range(actual[1]) for x in range(actual[0])]
        assert 0 in alpha and max(alpha) == 255 and sum(a>0 for a in alpha)>actual[0]*actual[1]*0.6, 'Blank or excessively padded icon'
        sizes.append(actual[0])
        if actual[0] == 256:
            chosen = image
assert set(sizes) == {16,20,24,32,48,64,128,256}, 'Missing native icon sizes'
manifest = leaf(resource_ids(24)[0][1])
assert b'asInvoker' in manifest and b'PerMonitorV2' in manifest, 'Missing DPI/no-elevation manifest'
version = leaf(resource_ids(16)[0][1])
assert 'WifiMap'.encode('utf-16le') in version, 'Missing product identity'
assert b'WifiMap third-party notices' in data and b'MIT License' in data, 'Missing embedded license notices'
imports = []
at = offset(u32(optional+112+8))
for _ in range(512):
    if data[at:at+20] == bytes(20):
        break
    name_at = offset(u32(at+12))
    imports.append(data[name_at:data.index(0,name_at)].decode('ascii').lower())
    at += 20
assert not set(imports)&{'opengl32.dll','d3d11.dll','d3d12.dll','dxgi.dll','vulkan-1.dll'}, 'Unexpected graphics API import'
assert not any(name.startswith(('vcruntime','msvcp140','concrt')) for name in imports), 'Unexpected redistributable CRT dependency'
report = {'artifact':args.exe.name,'architecture':'Windows x64','subsystem':'GUI','bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'icon_sizes':sorted(sizes),'manifest':'asInvoker, PerMonitorV2','embedded_licenses':True,'imports':sorted(imports),'scope':'Static PE evidence only. No application, OS icon or rendering runtime was launched.'}
if args.report:
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
if args.icon_output:
    args.icon_output.parent.mkdir(parents=True, exist_ok=True)
    args.icon_output.write_bytes(chosen)
print(json.dumps(report,indent=2))

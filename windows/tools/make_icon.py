"""Rebuild the original floor-plan/radio identity without external packages."""
from pathlib import Path
import math
import struct
import zlib

ROOT = Path(__file__).resolve().parents[1]

def png(size):
    rows = []
    for y in range(size):
        row = bytearray()
        for x in range(size):
            colors = []
            for sy in range(4):
                for sx in range(4):
                    u, v = (x+(sx+.5)/4)/size, (y+(sy+.5)/4)/size
                    dx, dy = max(abs(u-.5)-.33, 0), max(abs(v-.5)-.33, 0)
                    if dx*dx+dy*dy > .12*.12:
                        colors.append((0,0,0,0)); continue
                    color = (22,62,77,255)
                    # A square apartment corner, doorway, and radio rings.
                    if (.21 < u < .27 and .24 < v < .78) or (.21 < u < .76 and .72 < v < .78) or (.21 < u < .45 and .24 < v < .30):
                        color = (238,247,244,255)
                    r = math.hypot(u-.62,v-.56)
                    if v < .56 and u > .33 and (abs(r-.15)<.027 or abs(r-.30)<.027):
                        color = (87,216,187,255)
                    if math.hypot(u-.62,v-.57) < .043:
                        color = (248,203,107,255)
                    colors.append(color)
            row.extend(round(sum(c[i] for c in colors)/16) for i in range(4))
        rows.append(b'\0'+row)
    def chunk(name, data):
        return struct.pack('>I',len(data))+name+data+struct.pack('>I',zlib.crc32(name+data)&0xffffffff)
    return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',size,size,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(b''.join(rows),9))+chunk(b'IEND',b'')

sizes = [16,20,24,32,48,64,128,256]
images = [png(s) for s in sizes]
offset = 6+16*len(sizes)
entries = []
for size, data in zip(sizes, images):
    entries.append(struct.pack('<BBBBHHII',size%256,size%256,0,0,1,32,len(data),offset))
    offset += len(data)
(ROOT/'assets'/'wifi-map.ico').write_bytes(struct.pack('<HHH',0,1,len(sizes))+b''.join(entries)+b''.join(images))
(ROOT/'assets'/'icon.png').write_bytes(images[-1])

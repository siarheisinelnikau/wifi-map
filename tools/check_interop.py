from pathlib import Path
import json
ROOT = Path(__file__).resolve().parents[1]
fixture = json.loads((ROOT/'shared/fixtures/mixed-platform.wifimap').read_text())
windows = json.loads((ROOT/'windows/target/interop/windows-roundtrip.wifimap').read_text())
android = json.loads((ROOT/'android/domain/build/interop/android-roundtrip.wifimap').read_text())
assert fixture == windows == android, 'Portable project changed across platforms'
left = json.loads((ROOT/'windows/target/interop/windows-heat.json').read_text())
right = json.loads((ROOT/'android/domain/build/interop/android-heat.json').read_text())
assert len(left) == len(right)
for i, (a,b) in enumerate(zip(left,right)):
    assert (a is None) == (b is None), f'Support mismatch: {i}'
    if a is not None:
        assert abs(a-b) <= 0.001, f'Value mismatch: {i}: {a} vs {b}'
print('Both project round trips match the shared fixture; heatmap support and dBm agree within 0.001.')

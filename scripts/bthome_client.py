# /// script
# dependencies = ["bleak"]
# ///
"""Listen for BTHome v2 advertisements (service data UUID 0xFCD2) and decode them.

Talks to the ble_bthome example. Run: uv run scripts/bthome_client.py
"""
import asyncio
import struct
from bleak import BleakScanner

BTHOME_UUID = "0000fcd2-0000-1000-8000-00805f9b34fb"

# object id -> (name, struct format, factor, unit); only the ones the example sends plus a few common ones
OBJECTS = {
    0x00: ("packet id", "<B", 1, ""),
    0x01: ("battery", "<B", 1, "%"),
    0x02: ("temperature", "<h", 0.01, "C"),
    0x03: ("humidity", "<H", 0.01, "%"),
    0x04: ("pressure", "<I", 0.01, "hPa"),  # 3 bytes on the wire, handled below
    0x0C: ("voltage", "<H", 0.001, "V"),
}
SIZES = {0x00: 1, 0x01: 1, 0x02: 2, 0x03: 2, 0x04: 3, 0x0C: 2}


def decode(data: bytes) -> list[str]:
    info = data[0]
    version = info >> 5
    if version != 2 or info & 1:
        return [f"unsupported device info 0x{info:02x} (version {version}, encrypted={info & 1})"]
    out, i = [], 1
    while i < len(data):
        obj = data[i]
        if obj not in OBJECTS:
            out.append(f"unknown object 0x{obj:02x}, stopping")
            break
        name, fmt, factor, unit = OBJECTS[obj]
        size = SIZES[obj]
        raw = data[i + 1 : i + 1 + size].ljust(struct.calcsize(fmt), b"\0")
        value = struct.unpack(fmt, raw)[0] * factor
        out.append(f"{name}: {value:g} {unit}".strip())
        i += 1 + size
    return out


async def main():
    seen = set()
    print("Listening 30 s for BTHome advertisements...")

    def on_adv(device, ad):
        payload = ad.service_data.get(BTHOME_UUID)
        if payload is None:
            return
        key = (device.address, bytes(payload))
        if key in seen:
            return
        seen.add(key)
        print(f"{ad.local_name or device.address}: " + ", ".join(decode(bytes(payload))))

    async with BleakScanner(on_adv):
        await asyncio.sleep(30)


asyncio.run(main())

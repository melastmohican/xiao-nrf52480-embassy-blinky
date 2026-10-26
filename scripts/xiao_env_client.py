# /// script
# dependencies = ["bleak"]
# ///
"""Find XIAO-AHT30-*, connect, read and subscribe to temperature/humidity.

Talks to the ble_peripheral example. Run: uv run scripts/xiao_env_client.py
"""
import asyncio
import struct
from bleak import BleakClient, BleakScanner

TEMP = "00002a6e-0000-1000-8000-00805f9b34fb"
HUM = "00002a6f-0000-1000-8000-00805f9b34fb"


async def main():
    print("Scanning 10 s for XIAO-AHT30-*...")
    # macOS caches device names, so match on the name from the advertisement itself.
    found = {}

    def match(d, ad):
        if (ad.local_name or "").startswith("XIAO-AHT30"):
            found["name"] = ad.local_name
            return True
        return False

    dev = await BleakScanner.find_device_by_filter(match, timeout=10)
    if dev is None:
        print("Not found. Is the board running ble_peripheral and not already connected to a phone?")
        return
    print(f"Found {found['name']} ({dev.address})")

    async with BleakClient(dev) as client:
        t = struct.unpack("<h", await client.read_gatt_char(TEMP))[0] / 100
        h = struct.unpack("<H", await client.read_gatt_char(HUM))[0] / 100
        print(f"Read: {t:.2f} C, {h:.2f} %")

        await client.start_notify(TEMP, lambda _, d: print(f"Notify temperature: {struct.unpack('<h', d)[0] / 100:.2f} C"))
        await client.start_notify(HUM, lambda _, d: print(f"Notify humidity:    {struct.unpack('<H', d)[0] / 100:.2f} %"))
        await asyncio.sleep(15)


asyncio.run(main())

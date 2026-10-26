MEMORY
{
  /* The MBR and the S140 v7.3.0 SoftDevice live below 0x27000. The Matter stack does not enable the SoftDevice (BLE and the
   * radio come from nrf-sdc and MPSL, linked into the app), but the MBR starts the app and forwards its interrupts.
   *
   * The app ends at 0xED000: the Adafruit-style layout (also used by the XIAO) keeps 28 KB of user data (LittleFS) at 0xED000 and the DFU bootloader at
   * 0xF4000. The limit makes the linker refuse an app that would run into either of them, instead of the flash silently
   * overwriting the bootloader.
   */
  FLASH (rx) : ORIGIN = 0x00027000, LENGTH = 0xED000 - 0x27000

  /* S140 v7.3.0 reserves the first 0x3338 bytes of RAM. */
  RAM (rwx) : ORIGIN = 0x20003338, LENGTH = 0x20040000 - 0x20003338
}

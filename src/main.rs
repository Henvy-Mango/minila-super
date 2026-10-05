#![no_main]
#![no_std]

use rmk::macros::rmk_keyboard;

#[cfg(not(feature = "mpr121"))]
#[rmk_keyboard]
mod keyboard {}

#[cfg(feature = "mpr121")]
mod touch;

#[cfg(feature = "mpr121")]
#[rmk_keyboard]
mod keyboard {
    #[Override(entry)]
    async fn entry() {
        use embassy_rp::{
            gpio::{Input, Pull},
            i2c::{Config, I2c},
        };
        // MPR121 wiring: SDA=GP12, SCL=GP13, IRQ=GP14.
        let mut config = Config::default();
        config.frequency = 100_000;
        let bus = I2c::new_async(p.I2C0, p.PIN_13, p.PIN_12, crate::touch::MprIrqs, config);
        let mut touch_input = crate::touch::MprInput::new(bus, Input::new(p.PIN_14, Pull::Up));
        let mut touch_processor = crate::touch::processor::TouchProcessor::new();
        let mut usb_transport = rmk::usb::UsbTransport::new(driver, rmk_config.device_config)
            .with_host_service(&host_service);
        let mut wpm_processor = rmk::processor::builtin::wpm::WpmProcessor::new();
        // RMK starts the hardware watchdog before this overridden entry.
        // Keep its generated runner alive or RP2040 resets after 8 seconds.
        rmk::run_all!(
            matrix,
            encoder_0,
            storage,
            keyboard,
            usb_transport,
            wpm_processor,
            watchdog_runner,
            touch_processor,
            touch_input
        )
        .await;
    }
}

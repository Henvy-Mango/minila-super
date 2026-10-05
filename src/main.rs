#![no_main]
#![no_std]

use rmk::macros::rmk_keyboard;

mod touch_strip;

#[rmk_keyboard]
mod keyboard {
    #[Override(entry)]
    async fn entry() {
        // Replace None with Some([OUT0, ..., OUT5] GPIO inputs),
        // ordered bottom-to-top, once the board's GPIO assignment is known.
        let touch_pins: Option<[embassy_rp::gpio::Input<'static>; 6]> = None;
        let mut touch_input = touch_pins.map(crate::touch_strip::Xw06a::new);
        let mut touch_processor = crate::touch_strip::TouchStripProcessor::new();
        let mut usb_transport = rmk::usb::UsbTransport::new(driver, rmk_config.device_config)
            .with_host_service(&host_service);
        let mut wpm_processor = rmk::processor::builtin::wpm::WpmProcessor::new();

        // An entry override owns the complete runtime task list, including the encoder.
        rmk::run_all!(
            matrix,
            encoder_0,
            storage,
            keyboard,
            usb_transport,
            wpm_processor,
            touch_processor,
            touch_input
        )
        .await;
    }
}

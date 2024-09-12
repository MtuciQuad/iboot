#![cfg(feature = "ifrc_iflight_f722_blitz")]

use assign_resources::assign_resources;
use embassy_executor::task;
use embassy_stm32::peripherals::USB_OTG_FS;
use embassy_stm32::time::mhz;
use embassy_stm32::usb::Driver;
use embassy_stm32::{bind_interrupts, peripherals, Config};
use embassy_stm32::{usb, Peripherals};
use static_cell::StaticCell;

pub const FLASH_START: u32 = 64 * 1024; // 0x10000u32
pub const FLASH_END: u32 = FLASH_START + 0x10000u32 + 0x60000u32;

bind_interrupts!(pub struct Irqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

pub fn create_peripherals() -> Peripherals {
    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;
        config.enable_debug_during_sleep = true;
        config.rcc.hse = Some(Hse {
            freq: mhz(8),
            mode: HseMode::Oscillator,
        });
        config.rcc.pll_src = PllSource::HSE;
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV4,
            mul: PllMul::MUL216,
            divp: Some(PllPDiv::DIV2),
            divq: Some(PllQDiv::DIV9),
            divr: None,
        });
        config.rcc.pllsai = Some(Pll {
            prediv: PllPreDiv::DIV4,
            mul: PllMul::MUL192,
            divp: Some(PllPDiv::DIV8),
            divq: None,
            divr: None,
        });
        config.rcc.plli2s = Some(Pll {
            prediv: PllPreDiv::DIV4,
            mul: PllMul::MUL192,
            divp: None,
            divq: None,
            divr: None,
        });
        config.rcc.sys = Sysclk::PLL1_P;
        config.rcc.ahb_pre = embassy_stm32::pac::rcc::vals::Hpre::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
    }

    embassy_stm32::init(config)
}

assign_resources! {
    led: LedRes {
        led: PC15
    },
    usb: UsbRes {
        usb: USB_OTG_FS,
        dp: PA12,
        dm: PA11,
    },
    flash: FlashRes {
        flash: FLASH,
    }
}

pub fn create_usb_driver(usbr: UsbRes) -> Driver<'static, USB_OTG_FS> {
    static EP_OUT_BUF_STATIC: StaticCell<[u8; 256]> = StaticCell::new();
    let ep_out_buffer = EP_OUT_BUF_STATIC.init([0u8; 256]);
    let mut config = embassy_stm32::usb::Config::default();
    // config.vbus_detection = true;
    config.vbus_detection = false;
    let driver = Driver::new_fs(usbr.usb, Irqs, usbr.dp, usbr.dm, ep_out_buffer, config);

    driver
}

#[task]
pub async fn usb_task(
    mut usb: embassy_usb::UsbDevice<
        'static,
        embassy_stm32::usb::Driver<'static, peripherals::USB_OTG_FS>,
    >,
) {
    usb.run().await;
}

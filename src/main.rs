#![no_std]
#![no_main]
#![feature(async_closure)]

use core::ptr::addr_of_mut;

use defmt::info;
use defmt_rtt as _;
use embassy_futures::select::{select, Either};
use embassy_stm32::time::mhz;
use embassy_stm32::usb::Driver;
use embassy_stm32::{bind_interrupts, peripherals, usb};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::Builder;
use panic_probe as _;

use embassy_executor::{task, Spawner};
use embassy_stm32::flash::Flash;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_time::{Duration, Timer};
use static_cell::StaticCell;

static mut USER_RESET: Option<extern "C" fn()> = None;

bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

static mut DEL: u64 = 50;

// THIS CONFIG IS FOR F411CE, IT MUST BE FIXED FOR ANOTHER CHIP
const FLASH_START: u32 = 0x10000u32;
const FLASH_END: u32 = FLASH_START + 65536u32 + 393216u32;

#[task]
async unsafe fn blink(mut led: Output<'static>) {
    loop {
        led.set_high();
        Timer::after(Duration::from_millis(DEL)).await;
        led.set_low();
        Timer::after(Duration::from_millis(DEL)).await;
    }
}

#[task]
async fn usb_task(
    mut usb: embassy_usb::UsbDevice<
        'static,
        embassy_stm32::usb::Driver<'static, peripherals::USB_OTG_FS>,
    >,
) {
    usb.run().await;
}

fn magic_mut_ptr() -> *mut u32 {
    extern "C" {
        #[link_name = "_dfu_magic"]
        static mut magic: u32;
    }

    addr_of_mut!(magic)
}

/// Read magic value to determine if
/// device must enter DFU mode.
fn get_uninit_val() -> u32 {
    unsafe { magic_mut_ptr().read_volatile() }
}

/// Erase magic value in RAM so that
/// DFU would be triggered only once.
fn clear_uninit_val() {
    unsafe { magic_mut_ptr().write_volatile(0) };
}

pub fn bootload(scb: &mut cortex_m::peripheral::SCB, address: u32) {
    unsafe {
        let sp = *(address as *const u32);
        let rv = *((address + 4) as *const u32);

        USER_RESET = Some(core::mem::transmute(rv));
        scb.vtor.write(address);
        #[allow(deprecated)]
        cortex_m::register::msp::write(sp);
        (USER_RESET.unwrap())();
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    {
        if get_uninit_val() == 0xB00110AD {
            clear_uninit_val();
            bootload(
                unsafe { &mut cortex_m::Peripherals::steal().SCB },
                0x08000000 + FLASH_START,
            )
        }
    }

    let p = {
        let mut config = embassy_stm32::Config::default();
        {
            use embassy_stm32::rcc::*;
            config.enable_debug_during_sleep = true;
            config.rcc.hse = Some(Hse {
                freq: mhz(25),
                mode: HseMode::Oscillator,
            });
            config.rcc.pll_src = PllSource::HSE;
            config.rcc.pll = Some(Pll {
                prediv: PllPreDiv::DIV25,
                mul: PllMul::MUL192,
                divp: Some(PllPDiv::DIV2),
                divq: Some(PllQDiv::DIV4),
                divr: None,
            });
            config.rcc.sys = Sysclk::PLL1_P;
            config.rcc.ahb_pre = embassy_stm32::pac::rcc::vals::Hpre::DIV1;
            config.rcc.apb1_pre = APBPrescaler::DIV2;
            config.rcc.apb2_pre = APBPrescaler::DIV1;
        }
        embassy_stm32::init(config)
    };

    info!("start");

    let led = Output::new(p.PC13, Level::High, Speed::Low);

    spawner.must_spawn(blink(led));

    static EP_OUT_BUF_STATIC: StaticCell<[u8; 256]> = StaticCell::new();
    let ep_out_buffer = EP_OUT_BUF_STATIC.init([0u8; 256]);
    let mut config = embassy_stm32::usb::Config::default();
    // config.vbus_detection = true;
    config.vbus_detection = false;
    let driver = Driver::new_fs(p.USB_OTG_FS, Irqs, p.PA12, p.PA11, ep_out_buffer, config);

    // Create embassy-usb Config
    let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("lab");
    config.product = Some("IBOOT");
    config.serial_number = Some("24022022");

    // Required for windows compatibility.
    // https://developer.nordicsemi.com/nRF_Connect_SDK/doc/1.9.1/kconfig/CONFIG_CDC_ACM_IAD.html#help
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    // let mut device_descriptor = [0; 256];
    static CONFIG_DESC_STATIC: StaticCell<[u8; 256]> = StaticCell::new();
    let config_descriptor = CONFIG_DESC_STATIC.init([0; 256]);
    static BOS_DESC_STATIC: StaticCell<[u8; 256]> = StaticCell::new();
    let bos_descriptor = BOS_DESC_STATIC.init([0; 256]);
    static CONTROL_STATIC: StaticCell<[u8; 4096]> = StaticCell::new();
    let control_buf = CONTROL_STATIC.init([0; 4096]);

    static STATE: StaticCell<State> = StaticCell::new();
    let state = STATE.init(State::new());

    let mut builder = Builder::new(
        driver,
        config,
        // &mut device_descriptor,
        config_descriptor,
        bos_descriptor,
        &mut [], // no msos descriptors
        control_buf,
    );

    let mut class = CdcAcmClass::new(&mut builder, state, 64);

    let usb = builder.build();

    spawner.must_spawn(usb_task(usb));

    Timer::after_millis(300).await;
    let mut f = Flash::new_blocking(p.FLASH);

    match select(
        Timer::after_secs(5),
        (async || {
            //
            loop {
                class.wait_connection().await;
                if let Ok(n) = class.read_packet(&mut [0; 64]).await {
                    return n;
                }
            }
        })(),
    )
    .await
    {
        Either::First(_) => unsafe {
            DEL = 100;

            *magic_mut_ptr() = 0xB00110AD;
            cortex_m::peripheral::SCB::sys_reset();
        },
        Either::Second(_) => {
            unsafe { DEL = 25 };
            class.wait_connection().await;
            class.write_packet(b"connected").await.unwrap();
            let mut data = [0u8; 64];
            class.wait_connection().await;
            class.read_packet(&mut data).await.unwrap();
            let size = u32::from_be_bytes(data[0..4].try_into().unwrap());

            info!("{}", size);

            if size <= FLASH_END - FLASH_START {
                f.blocking_erase(FLASH_START, FLASH_END).unwrap();
                class.write_packet(b"erased").await.unwrap();
            } else {
                class.write_packet(b"to_big").await.unwrap();
            }

            // TODO: make this align to 32 bytes
            // let size = size + ((4 - size % 4) % 4);

            for i in 0..=(size / 64) {
                // data = [0; 64];
                let mut data = AlignedBuffer([0u8; 64]);
                class.wait_connection().await;
                class.read_packet(&mut data.0).await.unwrap();

                match f.blocking_write(FLASH_START + (i * 64), &data.0) {
                    Ok(_) => {
                        info!("{}", i * 64);
                    }
                    Err(e) => {
                        defmt::error!("{}", e);
                        loop {
                            Timer::after_secs(1).await;
                        }
                    }
                }
                class.wait_connection().await;
                class.write_packet(&data.0).await.unwrap();
                class.write_packet(&[]).await.unwrap();
            }
            info!("all bytes downloaded");
            Timer::after_secs(1).await;
            unsafe {
                *magic_mut_ptr() = 0xB00110AD;
            }
            cortex_m::peripheral::SCB::sys_reset();
        }
    }
}

#[repr(align(32))]
pub struct AlignedBuffer<const N: usize>(pub [u8; N]);

impl<const N: usize> AsRef<[u8]> for AlignedBuffer<N> {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl<const N: usize> AsMut<[u8]> for AlignedBuffer<N> {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

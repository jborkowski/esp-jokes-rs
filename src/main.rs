#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![allow(clippy::upper_case_acronyms)]

extern crate alloc;
use core::{mem::MaybeUninit, str};
use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
    Config, Stack, StackResources,
};
use embassy_time::{Duration, Timer};
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use embedded_svc::wifi::{ClientConfiguration, Configuration, Wifi};
use embedded_text::alignment::HorizontalAlignment;
use embedded_text::style::HeightMode;
use embedded_text::style::TextBoxStyleBuilder;
use embedded_text::TextBox;
use esp_backtrace as _;
use esp_println::println;
use esp_wifi::wifi::{WifiController, WifiDevice, WifiEvent, WifiMode, WifiState};
use esp_wifi::{initialize, EspWifiInitFor};

#[cfg(feature = "esp32c3")]
pub use esp32c3_hal as hal;
#[cfg(feature = "esp32c6")]
pub use esp32c6_hal as hal;

use hal::{
    clock::ClockControl, embassy, gpio::*, peripherals::Peripherals, prelude::*, timer::TimerGroup,
    Rtc, IO,
};
use hal::{systimer::SystemTimer, Rng};

use embedded_graphics::draw_target::DrawTarget;
use reqwless::client::{HttpClient, TlsConfig, TlsVerify};
use reqwless::request::Method;
use static_cell::make_static;

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

#[global_allocator]
static ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();

fn init_heap() {
    const HEAP_SIZE: usize = 32 * 1024;
    static mut HEAP: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();

    unsafe {
        ALLOCATOR.init(HEAP.as_mut_ptr() as *mut u8, HEAP_SIZE);
    }
}

#[cfg(feature = "display-st7735")]
mod display {
    use super::*;
    use embedded_graphics::pixelcolor::Rgb565;
    use hal::peripherals::SPI2;
    use hal::spi::FullDuplexMode;
    use hal::Spi;
    use st7735_lcd::{self, ST7735};

    pub type SPI = Spi<'static, SPI2, FullDuplexMode>;
    pub type DISPLAY<'a> = ST7735<SPI, GpioPin<Output<PushPull>, 6>, GpioPin<Output<PushPull>, 7>>;

    pub type Color = Rgb565;
    pub const BACKGROUND: Color = Rgb565::BLACK;
    pub const TEXT: Color = Rgb565::RED;

    pub fn flush(_display: &mut DISPLAY) -> Result<(), ()> {
        // no-op
        Ok(())
    }
}

#[cfg(feature = "display-ssd1306")]
mod display {
    use super::*;
    use embedded_graphics::pixelcolor::BinaryColor;
    use hal::i2c::I2C;
    use hal::peripherals::I2C0;
    use ssd1306::prelude::I2CInterface;
    use ssd1306::{mode::BufferedGraphicsMode, size::DisplaySize128x64, Ssd1306};

    pub type SIZE = DisplaySize128x64;
    pub const SIZE: SIZE = DisplaySize128x64;
    pub type DISPLAY<'a> = Ssd1306<I2CInterface<I2C<'a, I2C0>>, SIZE, BufferedGraphicsMode<SIZE>>;

    pub type Color = BinaryColor;
    pub const BACKGROUND: Color = BinaryColor::Off;
    pub const TEXT: Color = BinaryColor::On;

    pub fn flush(display: &mut DISPLAY) -> Result<(), display_interface::DisplayError> {
        display.flush()
    }
}

use display::DISPLAY;

#[embassy_executor::main(entry = "hal::entry")]
async fn main(spawner: embassy_executor::Spawner) {
    init_heap();
    let peripherals = Peripherals::take();
    #[cfg(feature = "esp32c3")]
    let mut system = peripherals.SYSTEM.split();
    #[cfg(feature = "esp32c6")]
    let mut system = peripherals.PCR.split();

    let clocks = ClockControl::max(system.clock_control).freeze();

    #[cfg(feature = "esp32c3")]
    let mut rtc = Rtc::new(peripherals.RTC_CNTL);
    #[cfg(feature = "esp32c6")]
    let mut rtc = Rtc::new(peripherals.LP_CLKRST);

    let timer_group0 = TimerGroup::new(
        peripherals.TIMG0,
        &clocks,
        &mut system.peripheral_clock_control,
    );
    let mut wdt0 = timer_group0.wdt;
    let timer_group1 = TimerGroup::new(
        peripherals.TIMG1,
        &clocks,
        &mut system.peripheral_clock_control,
    );
    let mut wdt1 = timer_group1.wdt;

    // disable watchdog timers
    rtc.swd.disable();
    rtc.rwdt.disable();
    wdt0.disable();
    wdt1.disable();

    let mut rng = Rng::new(peripherals.RNG);
    let timer = SystemTimer::new(peripherals.SYSTIMER).alarm0;
    let init = initialize(
        EspWifiInitFor::Wifi,
        timer,
        rng,
        system.radio_clock_control,
        &clocks,
    )
    .unwrap();

    let (wifi, ..) = peripherals.RADIO.split();
    let (wifi_interface, controller) =
        esp_wifi::wifi::new_with_mode(&init, wifi, WifiMode::Sta).unwrap();

    embassy::init(&clocks, timer_group0.timer0);

    let dhcp4_config = embassy_net::DhcpConfig::default();
    let config = Config::dhcpv4(dhcp4_config);

    let seed = rng.random();

    let stack = &*make_static!(Stack::new(
        wifi_interface,
        config,
        make_static!(StackResources::<3>::new()),
        seed.into()
    ));

    let io = IO::new(peripherals.GPIO, peripherals.IO_MUX);
    let input = io.pins.gpio9.into_pull_up_input();

    hal::interrupt::enable(
        hal::peripherals::Interrupt::GPIO,
        hal::interrupt::Priority::Priority1,
    )
    .unwrap();

    use display::*;

    #[cfg(feature = "display-st7735")]
    let mut display: DISPLAY = {
        use hal::{Delay, Spi};

        let miso = io.pins.gpio6.into_push_pull_output(); // A0
        let rst = io.pins.gpio7.into_push_pull_output();

        let spi: SPI = Spi::new(
            peripherals.SPI2,
            io.pins.gpio1,
            io.pins.gpio2,                         // sda
            io.pins.gpio0.into_push_pull_output(), // dc not connected
            io.pins.gpio8,
            60u32.MHz(),
            hal::spi::SpiMode::Mode0,
            &mut system.peripheral_clock_control,
            &clocks,
        );

        let mut display = st7735_lcd::ST7735::new(spi, miso, rst, true, false, 160, 128);

        let mut delay = Delay::new(&clocks);
        display.init(&mut delay).unwrap();
        display
            .set_orientation(&st7735_lcd::Orientation::Landscape)
            .unwrap();
        display.set_offset(0, 0);
        display
    };

    #[cfg(feature = "display-ssd1306")]
    let mut display: DISPLAY = {
        use hal::i2c::I2C;
        use ssd1306::prelude::*;
        use ssd1306::rotation::DisplayRotation;
        use ssd1306::*;

        let sda = io.pins.gpio1;
        let scl = io.pins.gpio2;

        let i2c = I2C::new(
            peripherals.I2C0,
            sda,
            scl,
            400u32.kHz(),
            &mut system.peripheral_clock_control,
            &clocks,
        );

        let interface = I2CDisplayInterface::new(i2c);
        let mut display =
            Ssd1306::new(interface, SIZE, DisplayRotation::Rotate0).into_buffered_graphics_mode();

        display.init().unwrap();
        display
    };

    display.clear(display::BACKGROUND).unwrap();
    display::flush(&mut display).unwrap();

    spawner.spawn(connection_wifi(controller)).ok();
    spawner.spawn(net_task(stack)).ok();
    spawner.spawn(task(input, stack, seed.into(), display)).ok();
    //  spawner.spawn(get_joke(&stack)).ok();
}

#[embassy_executor::task]
async fn task(
    mut input: Gpio9<Input<PullUp>>,
    stack: &'static Stack<WifiDevice<'static>>,
    seed: u64,
    mut display: DISPLAY<'static>,
) {
    let mut rx_buffer = [0; 8 * 1024];
    let mut tls_read_buffer = [0; 8 * 1024];
    let mut tls_write_buffer = [0; 8 * 1024];
    let client_state = TcpClientState::<4, 4096, 4096>::new();
    let tcp_client = TcpClient::new(stack, &client_state);
    let dns = DnsSocket::new(stack);

    let style = MonoTextStyle::new(&FONT_6X10, display::TEXT);
    let textbox_style = TextBoxStyleBuilder::new()
        .height_mode(HeightMode::FitToText)
        .alignment(HorizontalAlignment::Justified)
        .paragraph_spacing(6)
        .build();

    let bounds = Rectangle::new(
        Point::zero(),
        Size::new(display.bounding_box().size.width, 0),
    );

    loop {
        let _ = input.wait_for_any_edge().await;
        if input.is_high().unwrap() {
            println!("started");
            loop {
                if stack.is_link_up() {
                    break;
                }
                Timer::after(Duration::from_millis(500)).await;
            }

            println!("Waiting to get IP address...");
            loop {
                if let Some(config) = stack.config_v4() {
                    println!("Got IP: {}", config.address);
                    break;
                }
                Timer::after(Duration::from_millis(500)).await;
            }
            display.clear(display::TEXT).unwrap();
            display::flush(&mut display).unwrap();

            let tls_config = TlsConfig::new(
                seed,
                &mut tls_read_buffer,
                &mut tls_write_buffer,
                TlsVerify::None,
            );
            let mut http_client = HttpClient::new_with_tls(&tcp_client, &dns, tls_config);
            let mut request = http_client
                .request(
                    Method::GET,
                    "https://v2.jokeapi.dev/joke/Programming?format=txt",
                )
                .await
                .unwrap();

            let response = request.send(&mut rx_buffer).await.unwrap();

            let text = str::from_utf8(response.body().read_to_end().await.unwrap()).unwrap();
            println!("Joke: {}", text);

            let text_height = textbox_style.measure_text_height(&style, text, bounds.size.width);
            let screen_height = display.bounding_box().size.height;
            let max_offset = core::cmp::max(0, text_height as i32 - screen_height as i32) as u32;

            let mut offset: u32 = 0;
            loop {
                println!("Drawing at offset {offset}");

                display.clear(display::BACKGROUND).unwrap();
                TextBox::with_textbox_style(text, bounds, style, textbox_style)
                    .translate(Point::new(0, -(offset as i32)))
                    .draw(&mut display).unwrap();
                display::flush(&mut display).unwrap();

                if offset >= max_offset {
                    println!("Beyond max offset, end of this joke");
                    break;
                }

                println!("Still some text left to display (max_offset={max_offset}), press to scroll");
                offset = core::cmp::min(max_offset, offset + screen_height / 2);

                loop {
                    let _ = input.wait_for_any_edge().await;
                    if input.is_high().unwrap() {
                        break;
                    }
                }
            }
        }
    }
}

#[embassy_executor::task]
async fn connection_wifi(mut controller: WifiController<'static>) {
    println!("Start connect with wifi (SSID: {:?}) task", SSID);
    loop {
        if matches!(esp_wifi::wifi::get_wifi_state(), WifiState::StaConnected) {
            controller.wait_for_event(WifiEvent::StaDisconnected).await;
            Timer::after(Duration::from_millis(5000)).await
        }
        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: SSID.into(),
                password: PASSWORD.into(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            println!("Starting wifi");
            controller.start().await.unwrap();
            println!("Wifi started!");
        }

        match controller.connect().await {
            Ok(_) => println!("Wifi connected!"),
            Err(e) => {
                println!("Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

#[embassy_executor::task]
async fn net_task(stack: &'static Stack<WifiDevice<'static>>) {
    stack.run().await
}

#[embassy_executor::task]
async fn get_joke(stack: &'static Stack<WifiDevice<'static>>) {
    loop {
        if stack.is_link_up() {
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    println!("Waiting to get IP address...");
    loop {
        if let Some(config) = stack.config_v4() {
            println!("Got IP: {}", config.address);
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    println!("Got IP: {}", stack.is_config_up());
}

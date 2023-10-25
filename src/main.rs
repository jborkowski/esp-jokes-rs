#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

extern crate alloc;
use core::{mem::MaybeUninit, str::from_utf8};
use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
    Config, Stack, StackResources,
};
use embassy_net::{Ipv4Address, Ipv4Cidr};
use embassy_time::{Duration, Timer};
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use embedded_graphics::{pixelcolor::Rgb565, prelude::RgbColor};
use embedded_svc::wifi::{ClientConfiguration, Configuration, Wifi};
use esp_backtrace as _;
use esp_println::println;
use esp_wifi::wifi::{WifiController, WifiDevice, WifiEvent, WifiMode, WifiState};
use esp_wifi::{initialize, EspWifiInitFor};
use hal::peripherals::SPI2;
use hal::spi::FullDuplexMode;
use hal::{
    clock::ClockControl, embassy, gpio::*, peripherals::Peripherals, prelude::*, timer::TimerGroup,
    Delay, Rtc, Spi, IO,
};
use hal::{systimer::SystemTimer, Rng};

use embedded_graphics::draw_target::DrawTarget;
use reqwless::client::{HttpClient, TlsConfig, TlsVerify};
use reqwless::request::Method;
use static_cell::make_static;

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");
const IP_ADDRESS: Ipv4Cidr = Ipv4Cidr::new(Ipv4Address::new(192, 168, 50, 20), 24);
const GATEWAY: Ipv4Address = Ipv4Address::new(192, 168, 50, 1);

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
    use st7735_lcd::{self, ST7735};

    pub type SPI = Spi<'static, SPI2, FullDuplexMode>;
    pub type DISPLAY<'a> = ST7735<SPI, GpioPin<Output<PushPull>, 6>, GpioPin<Output<PushPull>, 7>>;

    pub type Color = Rgb565;
    pub const BACKGROUND: Color = Rgb565::BLACK;
    pub const TEXT: Color = Rgb565::RED;
}

#[cfg(feature = "display-ssd1306")]
mod display {
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
}

use display::DISPLAY;

#[embassy_executor::main(entry = "hal::entry")]
async fn main(spawner: embassy_executor::Spawner) {
    init_heap();
    let peripherals = Peripherals::take();
    let mut system = peripherals.SYSTEM.split();
    let clocks = ClockControl::max(system.clock_control).freeze();

    let mut rtc = Rtc::new(peripherals.RTC_CNTL);
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
            100u32.kHz(),
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

    spawner.spawn(connection_wifi(controller)).ok();
    spawner.spawn(net_task(&stack)).ok();
    spawner
        .spawn(task(input, &stack, seed.into(), display))
        .ok();
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
    let tcp_client = TcpClient::new(&stack, &client_state);
    let dns = DnsSocket::new(&stack);

    let style = MonoTextStyle::new(&FONT_6X10, display::TEXT);

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
            println!("clicked");

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

            let body = from_utf8(response.body().read_to_end().await.unwrap()).unwrap();

            display.clear(display::BACKGROUND).unwrap();

            Text::new(body, Point::new(10, 10), style)
                .draw(&mut display)
                .unwrap();
            println!("Http body: {}", body);
            Timer::after(Duration::from_millis(3000)).await;
        }
    }
}

#[embassy_executor::task]
async fn connection_wifi(mut controller: WifiController<'static>) {
    println!("Strat connect with wifi (SSID: {:?}) task", SSID);
    loop {
        match esp_wifi::wifi::get_wifi_state() {
            WifiState::StaConnected => {
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                Timer::after(Duration::from_millis(5000)).await
            }
            _ => {}
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

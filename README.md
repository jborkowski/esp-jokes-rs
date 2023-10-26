# ESP-Jokes-RS

ESP-Jokes-RS is a lightweight, fun, and educational project that brings humor and laughter to your ESP32C32 development board by displaying jokes on its OLED or LCD screen. What's unique about this project is that it is written in Rust, using the `no_std` library, in adherence to the principles described in the [ESP-RS book](https://esp-rs.github.io/book/writing-your-own-application/nostd.html). This repository is based on the Wokwi example project for ESP32 and aims to demonstrate how you can program your ESP32 board in Rust without relying on the standard library. 

But what makes ESP-Jokes-RS even more entertaining is that it fetches jokes from the internet on a button click! With the press of a button, your ESP32 board connects to https://v2.jokeapi.dev/ and retrieves a random joke from the internet, providing you with a source of endless laughter and amusement. This feature adds an interactive and dynamic element to your development board, making it both an educational and entertaining gadget.

## Features

- Displays a collection of jokes on the OLED or LCD screen of your ESP32.
- Written in Rust, showcasing how to use the `no_std` library for ESP32 development.
- Supports various display drivers, including ST7735 and SSD1306, thanks to the contributions in [this pull request](https://github.com/jborkowski/esp-jokes-rs/pull/1).

## Supported hardware configurations

Hardware options are selected using Cargo features. Some are selected by default. To change the configuration pass `--no-default-features --features <your_features>`.

Boards:

- `esp32c3`
- `esp32c6`

Displays:

- `display-st7735`
- `display-ssd1306`

- To build in the default mode (ESP32C3, ST7735), simply run:

   ```bash
   cargo run
   ```

- For example, to build on ESP32C6 with SSD1306 display, use the following command:

   ```bash
   cargo run --no-default-features --features esp32c6,display-ssd1306
   ```

## Getting Started

To get started with this project, you will need:

- An ESP32 development board (See [Supported hardware configurations](#supported-hardware-configurations) above for which boards are supported)
- An OLED or LCD display connected to your ESP32.
- Rust and the associated toolchain installed on your development machine.

Follow these steps to set up and run the project:

1. Clone this repository to your development machine.

   ```bash
   git clone https://github.com/jborkowski/esp-jokes-rs.git
   ```

2. Change the working directory to the project folder.

   ```bash
   cd esp-jokes-rs
   ```

3. Build the project using Cargo, the Rust package manager.

   ```bash
   cargo build
   ```

4. Flash the resulting binary to your ESP32 board.

   ```bash
   cargo run
   ```
   **Note**: You might need additional options here depending on your hardware configuration, see [Supported hardware configurations](#supported-hardware-configurations).

5. Once the flashing process is complete, press the BOOT button, and your ESP32 board will display a series of jokes on the OLED or LCD screen. Enjoy the humor!

## Project Structure

- `src/` contains the Rust source code for the project.
- `Cargo.toml` is the configuration file for the project, where dependencies and build options are specified.

## Contributing

We welcome contributions to this project. If you have ideas for more improvements or want to add features, feel free to open an issue or a pull request.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- This project is based on the Wokwi example project, and we are grateful for the inspiration it provided.

## Contributors

- [Jonatan Borkowski](https://github.com/jborkowski)
- [Maciej Bielecki](https://github.com/zyla)

## Special Thanks

Special thanks to the ESP32 community, the Rust programming language developers, and everyone who contributed to the libraries and tools used in this project.

---

Let's have some fun and share a good laugh with your ESP32 board using ESP-Jokes-RS! If you encounter any issues or have suggestions, please don't hesitate to get involved in this project. Happy coding and happy laughing!

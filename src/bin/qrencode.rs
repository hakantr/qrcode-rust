use std::env;
use std::process::ExitCode;

pub fn main() -> ExitCode {
    let Some(arg) = env::args().nth(1) else {
        eprintln!("kullanım: qrencode <metin>");
        return ExitCode::FAILURE;
    };

    let code = match qrcode::QrCode::new(arg.as_bytes()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("qrencode: {arg:?} kodlanamıyor: {error}");
            return ExitCode::FAILURE;
        }
    };

    match code.render().dark_color("\x1b[7m  \x1b[0m").light_color("\x1b[49m  \x1b[0m").try_build() {
        Ok(image) => {
            print!("{image}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("qrencode: çizilemiyor: {error}");
            ExitCode::FAILURE
        }
    }
}

use std::env;
use std::process::ExitCode;

pub fn main() -> ExitCode {
    let Some(arg) = env::args().nth(1) else {
        eprintln!("usage: qrencode <text>");
        return ExitCode::FAILURE;
    };

    let code = match qrcode::QrCode::new(arg.as_bytes()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("qrencode: cannot encode {arg:?}: {error}");
            return ExitCode::FAILURE;
        }
    };

    match code.render().dark_color("\x1b[7m  \x1b[0m").light_color("\x1b[49m  \x1b[0m").try_build() {
        Ok(image) => {
            print!("{image}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("qrencode: cannot render: {error}");
            ExitCode::FAILURE
        }
    }
}

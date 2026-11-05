//! `phone …` — the hub's iPhone path from a console (stage 8, check П4; D-132, D-139, D-143).
//!
//! ```text
//! sirtone-cli phone list
//! sirtone-cli phone login <apple-id>              once, in the owner's own console (password, 2FA)
//! sirtone-cli phone adopt <apple-id>              take over the П4 spike's identity (same certificate)
//! sirtone-cli phone install <apple-id> <udid|usb> <App.app|.ipa>   sign + install over the cable
//! sirtone-cli phone forget <apple-id>             delete everything kept for this Apple ID
//! ```
//!
//! Developer harness: it has no offline switch (the app checks it before calling devicelink).
//! It installs no log subscriber, so `isideload`'s lines go nowhere. Prints codes, not prose.

use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

use sirtone_devicelink::apple::{self, TwoFactorAnswer, TwoFactorAsk};
use sirtone_devicelink::{Link, Platform};

fn runtime() -> Option<tokio::runtime::Runtime> {
    // TLS for the anisette websocket: one process-wide provider (ignored if already set).
    let _ = rustls::crypto::ring::default_provider().install_default();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .ok()
}

fn fail(e: &sirtone_devicelink::LinkError) -> ExitCode {
    eprintln!("{}: {e}", e.code());
    ExitCode::FAILURE
}

fn line(prompt: &str) -> String {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    s.trim().to_string()
}

pub fn login(apple_id: &str) -> ExitCode {
    if !std::io::stdin().is_terminal() {
        // The password is typed by the owner only: never wait on a missing console.
        eprintln!("login.no_console");
        return ExitCode::FAILURE;
    }
    let Ok(password) = rpassword::prompt_password(format!("Apple ID password ({apple_id}): "))
    else {
        eprintln!("login.no_console");
        return ExitCode::FAILURE;
    };
    let Some(rt) = runtime() else {
        return ExitCode::FAILURE;
    };
    let ask = |a: TwoFactorAsk| async move {
        if a.retry {
            println!("The code or method did not work.");
        }
        println!(
            "{}",
            if a.sms {
                "Code sent by SMS."
            } else {
                "Code shown on your Apple devices."
            }
        );
        match line("6 digits (d = to devices, r = resend, empty = cancel): ").as_str() {
            "d" => TwoFactorAnswer::SendToDevices,
            "r" => TwoFactorAnswer::Resend,
            "" => TwoFactorAnswer::Abort,
            c => TwoFactorAnswer::Code(c.to_string()),
        }
    };
    let r = rt.block_on(apple::login(apple_id, &password, ask));
    drop(password);
    match r {
        Ok(s) => {
            println!("signed in; token until {} (unix ms)", s.token_expiry_ms);
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e),
    }
}

pub fn adopt(apple_id: &str) -> ExitCode {
    match apple::adopt(apple::SPIKE_SERVICE, apple_id) {
        Ok(true) => {
            println!("adopted");
            ExitCode::SUCCESS
        }
        Ok(false) => {
            println!("nothing to adopt (already signed in, or no spike identity)");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e),
    }
}

pub fn install(apple_id: &str, phone: &str, app: &str) -> ExitCode {
    let Some(rt) = runtime() else {
        return ExitCode::FAILURE;
    };
    let udid = if phone == "usb" {
        let (phones, _) = rt.block_on(sirtone_devicelink::all_phones());
        match phones
            .into_iter()
            .find(|p| p.platform == Platform::Ios && p.link == Link::Usb)
        {
            Some(p) => p.id,
            None => {
                eprintln!("phone.device: no iPhone on the cable");
                return ExitCode::FAILURE;
            }
        }
    } else {
        phone.to_string()
    };
    match rt.block_on(apple::install(apple_id, &udid, Path::new(app), &|_| {})) {
        Ok(took) => {
            println!("installed on {udid} in {:.1} s", took.as_secs_f32());
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e),
    }
}

pub fn forget(apple_id: &str) -> ExitCode {
    match apple::forget(apple_id) {
        Ok(()) => {
            println!("forgotten");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e),
    }
}

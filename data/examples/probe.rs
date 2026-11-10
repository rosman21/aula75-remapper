//! Read-only connection check: finds the receiver (or cable), asks whether the keyboard is
//! online, and reads its lighting block and base layer. Writes nothing.
//!
//! cargo run -p aula75-data --example probe

use aula75_data::{hid, layout, protocol as p, session::Session};

fn main() {
    let mut api = hid::api().unwrap_or_else(|e| panic!("{e}"));
    let scan = hid::scan(&mut api).unwrap();
    println!("scan: {scan:?}");
    let Some(wire) = hid::open(&api).unwrap_or_else(|e| panic!("{e}")) else {
        println!("no AULA receiver or cable found");
        return;
    };
    println!(
        "opened {:?} ({})",
        wire.product,
        p::Link::name(aula75_data::session::Wire::link(&wire))
    );
    let mut s = Session::new(Box::new(wire));
    println!("online: {:?}", s.online());
    match s.led_block() {
        Ok(b) => {
            println!(
                "led block: {}",
                b.iter()
                    .map(|x| format!("{x:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            println!("lighting: {:?}", p::lighting_from_block(&b));
        }
        Err(e) => println!("led block: {e}"),
    }
    for layer in 0..2 {
        match s.keymap(layer) {
            Ok(k) => {
                let named: Vec<String> = layout::keys()
                    .take(16)
                    .map(|key| {
                        format!(
                            "{}={}",
                            key.label,
                            aula75_data::keycodes::describe(
                                p::code_at(&k, key.index as usize),
                                &[]
                            )
                        )
                    })
                    .collect();
                println!(
                    "layer {layer}: plausible base={} {}",
                    layout::base_plausible(&k),
                    named.join(" ")
                );
            }
            Err(e) => println!("layer {layer}: {e}"),
        }
    }
}

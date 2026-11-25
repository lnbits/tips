mod api;
mod host;
mod input;

use serde_json::{json, Value};

wit_bindgen::generate!({ path: "../wasm/lnbits-extension.wit", world: "tips", generate_all });
struct Extension;

type Reply = Result<Value, String>;

fn run(input: &str, handler: fn(&Value) -> Reply) -> String {
    let request = if input.is_empty() {
        Ok(json!({}))
    } else {
        serde_json::from_str(input)
    };
    let result = match request {
        Ok(value) if value.is_object() => handler(&value),
        _ => Err("request must be a JSON object.".into()),
    };
    match result {
        Ok(data) => json!({"ok": true, "data": data}),
        Err(error) => json!({"ok": false, "error": error}),
    }
    .to_string()
}

// Each WIT export has the same JSON envelope.
macro_rules! routes {
    ($($name:ident),+ $(,)?) => {
        impl Guest for Extension {
            $(fn $name(input:String) -> String { run(&input, api::$name) })+
        }
    };
}
routes!(
    create_tip_jar,
    update_tip_jar,
    list_tip_jars,
    delete_tip_jar,
    list_tip_jar_tips,
    list_tip_wallets,
    list_tip_watchonly_wallets,
    get_bitcoin_rate,
    list_tip_currencies,
    get_public_tip_jar,
    create_tip_invoice,
    record_payment
);
export!(Extension);

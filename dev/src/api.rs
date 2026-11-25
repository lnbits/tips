use crate::lnbits::extension::{host as wit, utils_currencies as fx};
use crate::{host, input::*, Reply};
use serde_json::{json, Value};
const JARS: &str = "tip_jars";
const TIPS: &str = "tips";

pub fn create_tip_jar(req: &Value) -> Reply {
    save_jar(req, false)
}
pub fn update_tip_jar(req: &Value) -> Reply {
    save_jar(req, true)
}
pub fn list_tip_jars(req: &Value) -> Reply {
    let mut opts = page_options(req);
    opts["searchFields"] = json!([
        "title",
        "description",
        "wallet_name",
        "watchonly_wallet_name",
        "thank_you_message"
    ]);
    opts["sortBy"] = if req["sortBy"] == "title" {
        "title"
    } else {
        ""
    }
    .into();
    let p = host::page(JARS, opts)?;
    let jars = p.rows.iter().map(jar_view).collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"jars":jars,"total":p.total}))
}
pub fn delete_tip_jar(req: &Value) -> Reply {
    let id = required(&req["jarId"], "jarId", 128)?;
    jar(&id, false)?;
    if host::page(TIPS, json!({"filters":{"jar_id":id,"paid":true},"limit":1}))?.total > 0 {
        return Err("Tip jars with paid tips cannot be deleted.".into());
    }
    host::delete(JARS, &id)?;
    host::log(&format!("tips: deleted jar {id}"), "info");
    Ok(json!({"id":id,"deleted":true}))
}
pub fn list_tip_jar_tips(req: &Value) -> Reply {
    let id = required(&req["jarId"], "jarId", 128)?;
    let jar = jar(&id, false)?;
    let mut opts = page_options(req);
    opts["filters"] = json!({"jar_id":id});
    opts["searchFields"] = json!(["name", "message", "payment_hash"]);
    opts["sortBy"] = match s(&req["sortBy"]) {
        "amountSat" => "amount_sat",
        "jarTitle" => "jar_id",
        "name" => "name",
        "paid" => "paid",
        "paidAt" => "paid_at",
        "paymentHash" => "payment_hash",
        _ => "created_at",
    }
    .into();
    let p = host::page(TIPS, opts)?;
    let tips:Vec<_>=p.rows.iter().map(|t|{
        let mut v=json!({"id":t["id"],"jarId":t["jar_id"],"amountSat":t["amount_sat"],"name":t["name"],"message":t["message"],"paid":t["paid"],"createdAt":t["created_at"],"jarTitle":jar["title"],"paymentHash":t["payment_hash"]});
        if let Some(paid)=t.get("paid_at"){v["paidAt"]=paid.clone();} v
    }).collect();
    Ok(json!({"jar":jar_view(&jar)?,"tips":tips,"total":p.total}))
}
pub fn list_tip_wallets(_: &Value) -> Reply {
    Ok(json!({"wallets":host::wallets()}))
}
pub fn list_tip_watchonly_wallets(_: &Value) -> Reply {
    let values = watchonly("/api/v1/wallet?network=Mainnet")?;
    let rows=values.as_array().ok_or("Watchonly returned an invalid wallet list.")?.iter().map(|w|{
        let id=clean(&w["id"],128);let title=clean(&w["title"],120);let network=clean(&w["network"],32);
        json!({"id":id,"title":if title.is_empty(){&id}else{&title},"balance":num(&w["balance"],0.),"network":if network.is_empty(){"Mainnet"}else{&network}})
    }).collect::<Vec<_>>();
    Ok(json!({"wallets":rows}))
}
pub fn get_bitcoin_rate(_: &Value) -> Reply {
    let rate = fx::rate(&fx::CurrencyRateRequest {
        currency: "USD".into(),
    });
    if !rate.price.is_finite() || rate.price <= 0. {
        return Err("LNbits returned an invalid Bitcoin price.".into());
    }
    let (rates, error) = match external_rates() {
        Ok(r) => (r, String::new()),
        Err(e) => {
            host::log(&format!("tips: mempool rate unavailable: {e}"), "warning");
            (vec![], e)
        }
    };
    Ok(
        json!({"source":"LNbits","currency":"USD","btcUsd":rate.price,"satsPerUsd":rate.rate.round(),"sampleAmountSat":1000,"sampleAmountUsd":(rate.price/1000.).round()/100.,
        "fetchedAt":host::now(),"externalRates":rates,"externalRateError":error,"externalRateSource":"mempool.space"}),
    )
}
pub fn list_tip_currencies(_: &Value) -> Reply {
    Ok(json!({"currencies":currencies("sat")}))
}
pub fn get_public_tip_jar(req: &Value) -> Reply {
    let id = required(&req["jarId"], "jarId", 128)?;
    let row = jar(&id, true)?;
    Ok(
        json!({"jar":jar_view(&row)?,"tips":[],"currencies":currencies(string_or(&row["currency"],"sat"))}),
    )
}
pub fn create_tip_invoice(req: &Value) -> Reply {
    let id = required(&req["jarId"], "jarId", 128)?;
    let row = jar(&id, true)?;
    if row["payment_method"] == "onchain" || row["paymentMethod"] == "onchain" {
        return Err("Onchain tip jars do not create Lightning invoices.".into());
    }
    let currency = currency(first(&[&req["currency"], &row["currency"]]))?;
    let amount = amount(alias(req, "amount", "amountSat"), &currency)
        .ok_or("amount must be a positive value.")?;
    let name = clean(&req["name"], 60);
    let name = if name.is_empty() { "Anonymous" } else { &name };
    let message = clean(&req["message"], 280);
    let mut memo = s(&row["title"]).to_string();
    if !message.is_empty() {
        memo.push_str(": ");
        memo.push_str(&message);
    }
    let sat = if currency == "sat" {
        amount as u64
    } else {
        fx::fiat_to_sats(&fx::FiatToSatsRequest {
            amount,
            currency: currency.clone(),
        })
        .amount_sat
    };
    if sat == 0 {
        return Err("amount must convert to at least one sat.".into());
    }
    let tip = host::append_public(
        TIPS,
        &id,
        &json!({"amount_sat":sat,"name":name,"message":message,"payment_hash":"","paid":false,"created_at":host::now()}),
    );
    let mut invoice = host::invoice(
        &id,
        amount,
        &currency,
        &memo,
        vec![
            ("name".into(), name.into()),
            ("message".into(), message),
            ("tipId".into(), tip.clone()),
        ],
    );
    invoice["tipId"] = tip.into();
    Ok(invoice)
}
pub fn record_payment(event: &Value) -> Reply {
    let p = &event["payment"];
    let ex = &event["extra"];
    let pe = &p["extra"];
    let nested = if ex["extra_tips"].is_object() {
        &ex["extra_tips"]
    } else {
        &pe["extra_tips"]
    };
    let hash = required(
        first(&[
            &event["paymentHash"],
            &event["payment_hash"],
            &p["paymentHash"],
            &p["payment_hash"],
        ]),
        "paymentHash",
        128,
    )?;
    let tip_id = clean(
        first(&[
            &event["tipId"],
            &event["tip_id"],
            &ex["tipId"],
            &ex["tip_id"],
            &pe["tipId"],
            &pe["tip_id"],
            &nested["tipId"],
            &nested["tip_id"],
        ]),
        128,
    );
    let (tip, jar) = if !tip_id.is_empty() {
        let mut tip = host::get(TIPS, &tip_id)?;
        if tip.is_null() {
            return Ok(json!({"ok":false,"error":"payment not found"}));
        }
        if !s(&tip["payment_hash"]).is_empty() && tip["payment_hash"] != hash {
            return Ok(json!({"ok":false,"error":"payment does not match tip"}));
        }
        if tip["paid"] == true {
            return Ok(json!({"ok":true,"tipId":tip["id"],"paid":true,"alreadyPaid":true}));
        }
        let jar = jar(s(&tip["jar_id"]), false)?;
        merge(
            &mut tip,
            json!({"payment_hash":hash,"paid":true,"paid_at":host::now()}),
        );
        (tip, jar)
    } else {
        let source = clean(
            first(&[
                &event["sourceId"],
                &event["source_id"],
                &ex["sourceId"],
                &ex["source_id"],
                &pe["sourceId"],
                &pe["source_id"],
            ]),
            128,
        );
        if source.is_empty() {
            return Ok(json!({"ok":false,"error":"payment source not found"}));
        }
        let jar = jar(&source, false)?;
        // Legacy events lack a tip ID. Reuse a receipt on delivery retries.
        let receipts = host::page(
            TIPS,
            json!({"filters":{"jar_id":source,"payment_hash":hash,"paid":true},"limit":1}),
        )?;
        if let Some(receipt) = receipts.rows.first() {
            return Ok(json!({"ok":true,"tipId":receipt["id"],"paid":true,"alreadyPaid":true}));
        }
        let name = clean(&nested["name"], 60);
        let now = host::now();
        let msat = if i(&event["amount"]) != 0 {
            i(&event["amount"])
        } else {
            i(&p["amount"])
        };
        (
            json!({"id":host::id("tip"),"jar_id":source,"amount_sat":msat.unsigned_abs()/1000,
            "name":if name.is_empty(){"Anonymous"}else{&name},"message":clean(&nested["message"],280),"payment_hash":hash,"paid":true,"created_at":now,"paid_at":now}),
            jar,
        )
    };
    host::set(TIPS, &tip)?;
    let payment = platform_payment(&jar, &tip);
    host::log(
        &format!("tips: recorded paid tip {}", s(&tip["id"])),
        "info",
    );
    Ok(json!({"ok":true,"tipId":tip["id"],"paid":true,"platformPayment":payment}))
}
fn save_jar(req: &Value, update: bool) -> Reply {
    let id = if update {
        required(&req["jarId"], "jarId", 128)?
    } else {
        let id: String = s(&req["id"])
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            .take(64)
            .collect();
        if id.is_empty() {
            host::id("tipjar")
        } else {
            id
        }
    };
    let mut row = if update { jar(&id, false)? } else { json!({}) };
    let onchain = req["paymentMethod"] == "onchain";
    let currency = currency(&req["currency"])?;
    let mut wallet = String::new();
    let mut watch = String::new();
    let mut wallet_name = String::new();
    let mut watch_name = String::new();
    let mut address = String::new();
    if onchain {
        watch = required(&req["watchonlyWalletId"], "watchonlyWalletId", 128)?;
        watch_name = clean(&req["watchonlyWalletName"], 120);
        if watch_name.is_empty() {
            watch_name = watch.clone();
        }
        address = if row["watchonly_wallet_id"] == watch && !s(&row["onchain_address"]).is_empty() {
            s(&row["onchain_address"]).into()
        } else {
            required(
                &watchonly(&format!("/api/v1/address/{}", uri(&watch)))?["address"],
                "onchain address",
                256,
            )?
        };
    } else {
        wallet = required(&req["walletId"], "walletId", 128)?;
        wallet_name = clean(&req["walletName"], 120);
        if wallet_name.is_empty() {
            wallet_name = wallet.clone();
        }
    }
    let enabled = matches!(
        s(&req["platformSupportMode"]).trim(),
        "platform" | "pay_platform"
    ) || req["platformSupportEnabled"] == true;
    let (percent, lnurl) = if enabled {
        if onchain {
            return Err("Platform support is only available for Lightning jars.".into());
        }
        let n = integer(&req["platformSupportPercentage"], 0, 1, 100)
            .map_err(|_| "Platform support percentage must be an integer from 1 to 100.")?;
        (
            n,
            required(&req["platformSupportLnurl"], "platform LNURL", 2048)?,
        )
    } else {
        (0, String::new())
    };
    let title = clean(&req["title"], 80);
    let thanks = clean(&req["thankYouMessage"], 160);
    let mut amounts = Vec::new();
    for raw in req["suggestedAmounts"].as_array().into_iter().flatten() {
        if let Some(v) = amount(raw, &currency) {
            if !amounts.contains(&v) {
                amounts.push(v);
            }
            if amounts.len() == 6 {
                break;
            }
        }
    }
    let amounts = if amounts.is_empty() {
        if currency == "sat" {
            json!([100, 500, 1000])
        } else {
            json!([1, 5, 10])
        }
    } else {
        json!(amounts)
    };
    let now = host::now();
    if !update {
        let slug = slug(s(&req["slug"]));
        row["slug"] = if slug.is_empty() { id.clone() } else { slug }.into();
        row["created_at"] = now.into();
    }
    merge(
        &mut row,
        json!({"id":id,"title":if title.is_empty(){"Tip jar"}else{&title},"description":clean(&req["description"],280),
        "payment_method":if onchain{"onchain"}else{"lightning"},"currency":currency,"wallet_id":wallet,"wallet_name":wallet_name,
        "watchonly_wallet_id":watch,"watchonly_wallet_name":watch_name,"onchain_address":address,"suggested_amounts":amounts,
        "thank_you_message":if thanks.is_empty(){"Thanks for the tip."}else{&thanks},"platform_support_enabled":enabled,
        "platform_support_percentage":percent,"platform_support_lnurl":lnurl,"updated_at":now}),
    );
    host::set(JARS, &row)?;
    host::log(&format!("tips: saved jar {id}"), "info");
    jar_view(&row)
}
fn jar(id: &str, public: bool) -> Reply {
    let row = if public {
        host::get_public(JARS, id)?
    } else {
        host::get(JARS, id)?
    };
    if row.is_null() {
        Err("Tip jar not found.".into())
    } else {
        Ok(row)
    }
}
fn jar_view(j: &Value) -> Reply {
    let mut view = json!({"id":j["id"],"paymentMethod":string_or(&j["payment_method"],"lightning"),"currency":currency(&j["currency"])?,
        "walletId":s(&j["wallet_id"]),"watchonlyWalletId":s(&j["watchonly_wallet_id"]),"onchainAddress":s(&j["onchain_address"]),
        "platformSupportMode":if j["platform_support_enabled"]==true{"platform"}else{"none"},
        "platformSupportPercentage":i(&j["platform_support_percentage"]),"platformSupportLnurl":s(&j["platform_support_lnurl"])});
    for (key, field) in [
        ("title", "title"),
        ("description", "description"),
        (
            "walletName",
            if j["payment_method"] == "onchain" {
                "watchonly_wallet_name"
            } else {
                "wallet_name"
            },
        ),
        ("slug", "slug"),
        ("suggestedAmounts", "suggested_amounts"),
        ("thankYouMessage", "thank_you_message"),
        ("createdAt", "created_at"),
        ("updatedAt", "updated_at"),
    ] {
        if let Some(v) = j.get(field) {
            view[key] = v.clone();
        }
    }
    Ok(view)
}
fn currency(v: &Value) -> Result<String, String> {
    let c = clean(v, 8).to_uppercase();
    if matches!(c.as_str(), "" | "SAT" | "SATS") {
        Ok("sat".into())
    } else if fx::list_currencies().currencies.contains(&c) {
        Ok(c)
    } else {
        Err("currency is not supported.".into())
    }
}
fn currencies(default: &str) -> Vec<String> {
    let mut out = vec!["sat".into()];
    for c in std::iter::once(default.to_owned()).chain(fx::list_currencies().currencies) {
        if !c.is_empty() && !out.contains(&c) {
            out.push(c);
        }
    }
    out
}
fn amount(v: &Value, c: &str) -> Option<f64> {
    let n = num(v, 0.);
    if !n.is_finite() || n <= 0. || n > 1e7 || (c == "sat" && n.fract() != 0.) {
        None
    } else {
        format!("{n:.2}").parse().ok()
    }
}
fn watchonly(path: &str) -> Reply {
    let (status, body) = host::extension("watchonly", "GET", path, None);
    if status != 200 {
        return Err(format!("Watchonly returned HTTP {status}."));
    }
    serde_json::from_str(&body).map_err(|_| "Watchonly returned an invalid response.".into())
}
fn uri(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn slug(value: &str) -> String {
    let mut out = String::new();
    let mut invalid = false;
    for c in value.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
            if invalid {
                out.push('-');
                invalid = false;
            }
            out.push(c);
        } else {
            invalid = true;
        }
    }
    out.trim_matches('-').chars().take(64).collect()
}
fn page_options(req: &Value) -> Value {
    let (limit, offset) = pagination(req, 10);
    json!({"limit":limit,"offset":offset,"search":clean(&req["search"],256),"descending":req["descending"]==true||req["descending"]=="true"})
}
fn external_rates() -> Result<Vec<Value>, String> {
    let r = wit::http_request(&wit::HttpRequestOptions {
        method: "GET".into(),
        url: "https://mempool.space/api/v1/prices".into(),
        headers: vec![],
        body: None,
    });
    if r.status_code != 200 {
        return Err(format!("mempool.space returned HTTP {}.", r.status_code));
    }
    let data: Value = serde_json::from_str(&r.body).map_err(|_| "Invalid price response.")?;
    let time = i(&data["time"]);
    let time = if time > 0 { time as u64 } else { host::now() };
    Ok([("USD","🇺🇸"),("EUR","🇪🇺"),("GBP","🇬🇧"),("CAD","🇨🇦"),("CHF","🇨🇭"),("AUD","🇦🇺"),("JPY","🇯🇵")].into_iter().filter_map(|(currency,flag)|{
        let price=num(&data[currency],0.);(price.is_finite()&&price>0.).then(||json!({"source":"mempool.space","currency":currency,"flag":flag,"price":price,"fetchedAt":time}))
    }).collect())
}
fn platform_payment(j: &Value, t: &Value) -> Value {
    let percent = i(&j["platform_support_percentage"]);
    let amount = (i(&t["amount_sat"]) as i128 * percent as i128 / 100) as i64;
    let reason = if j["platform_support_enabled"] != true {
        "disabled"
    } else if string_or(&j["payment_method"], "lightning") != "lightning" {
        "not a Lightning jar"
    } else if s(&j["wallet_id"]).is_empty() {
        "missing wallet"
    } else if amount <= 0 {
        "amount too small"
    } else {
        ""
    };
    if !reason.is_empty() {
        return json!({"paid":false,"skipped":reason});
    }
    let memo = format!(
        "Platform support from {}",
        string_or(&j["title"], "tip jar")
    );
    let r = wit::pay_lnurl(&wit::PayLnurlRequest {
        wallet_id: s(&j["wallet_id"]).into(),
        lnurl: clean(&j["platform_support_lnurl"], 2048),
        amount: amount as f64,
        currency: "sat".into(),
        comment: Some(memo.clone()),
        description: memo,
        max_sat: Some(amount as u64),
        extra: vec![
            ("jarId".into(), s(&j["id"]).into()),
            ("tipId".into(), s(&t["id"]).into()),
            ("platformSupport".into(), "true".into()),
        ],
    });
    if r.ok {
        json!({"paid":true,"amountSat":amount,"paymentHash":r.payment_hash.unwrap_or_default(),"checkingId":r.checking_id.unwrap_or_default()})
    } else {
        json!({"paid":false,"error":r.error.unwrap_or_else(||"Platform support payment failed.".into())})
    }
}

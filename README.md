# tips

## Build the Rust component

```sh
rustup target add wasm32-wasip2
npm --prefix dev run check
npm --prefix dev run build:wasm
```

LNbits loads the resulting `wasm/module.wasm`.

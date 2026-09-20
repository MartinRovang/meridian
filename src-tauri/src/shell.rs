use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Nocturne's --color-bg. Without it the webview paints white for a frame before the splash
/// renders, which is a white flash on every launch of a dark app.
const BG: (u8, u8, u8, u8) = (0x16, 0x18, 0x26, 0xff);

/// Open the window and block until it closes.
///
/// The frontend gets its API base and token through the launch query rather than a config file:
/// the UI is on the asset protocol, so this query never leaves the machine, and local and remote
/// modes differ by nothing but these two values.
pub fn run(api_base: &str, token: &str) {
    let query = format!(
        "api={}&token={}",
        urlencoding::encode(api_base),
        urlencoding::encode(token)
    );
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.unminimize();
                let _ = main.show();
                let _ = main.set_focus();
            }
        }))
        .setup(move |app| {
            // ponytail: the window is built here, not in tauri.conf.json, because only here is
            // the query known. `windows` in the config stays empty on purpose; two window
            // definitions would race.
            let url = WebviewUrl::App(format!("index.html?{query}").into());
            WebviewWindowBuilder::new(app, "main", url)
                .title("Meridian")
                .inner_size(1400.0, 900.0)
                .center()
                .background_color(BG.into())
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri");
}

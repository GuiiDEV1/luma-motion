use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::{Icon, ResizeDirection, WindowBuilder},
};
use wry::{WebContext, WebViewBuilder};

pub fn run(
    url: &str,
    data: &std::path::Path,
    can_close: impl Fn() -> bool + 'static,
) -> Result<(), String> {
    let event_loop = EventLoopBuilder::<String>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let rgba = include_bytes!("../assets/icon.rgba").to_vec();
    let icon = Icon::from_rgba(rgba, 64, 64).map_err(|e| e.to_string())?;
    let window = WindowBuilder::new()
        .with_title("Luma Motion")
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(1280.0, 900.0))
        .with_min_inner_size(LogicalSize::new(900.0, 650.0))
        .with_theme(Some(tao::window::Theme::Dark))
        .with_window_icon(Some(icon))
        .build(&event_loop)
        .map_err(|e| e.to_string())?;
    let mut context = WebContext::new(Some(data.join("webview")));
    let trusted = url.to_string();
    let ipc_origin = trusted.clone();
    let webview = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(url)
        .with_ipc_handler(move |request| {
            if request
                .uri()
                .to_string()
                .starts_with(&format!("{ipc_origin}/"))
            {
                let _ = proxy.send_event(request.body().clone());
            }
        })
        .with_background_color((16, 19, 22, 255))
        .with_initialization_script("window.LUMA_DESKTOP = true;")
        .with_navigation_handler(move |uri| {
            uri.starts_with(&format!("{trusted}/")) || uri == trusted || uri.starts_with("blob:")
        })
        .with_download_started_handler(|_, destination| {
            let name = destination
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("luma-export.mp4");
            match rfd::FileDialog::new()
                .set_title("Salvar arquivo do Luma Motion")
                .set_file_name(name)
                .save_file()
            {
                Some(path) => {
                    *destination = path;
                    true
                }
                None => false,
            }
        })
        .build(&window)
        .map_err(|e| {
            format!(
                "Não foi possível abrir a janela. Verifique o Microsoft Edge WebView2 Runtime: {e}"
            )
        })?;
    event_loop.run(move |event, _, control_flow| {
        let _keep_alive = (&webview, &context, &window);
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(command) => match command.as_str() {
                "minimize" => window.set_minimized(true),
                "maximize" => window.set_maximized(!window.is_maximized()),
                "drag" => {
                    let _ = window.drag_window();
                }
                "close" => close_window(&can_close, control_flow),
                direction if direction.starts_with("resize:") => {
                    let direction = match &direction[7..] {
                        "n" => ResizeDirection::North,
                        "s" => ResizeDirection::South,
                        "e" => ResizeDirection::East,
                        "w" => ResizeDirection::West,
                        "ne" => ResizeDirection::NorthEast,
                        "nw" => ResizeDirection::NorthWest,
                        "se" => ResizeDirection::SouthEast,
                        "sw" => ResizeDirection::SouthWest,
                        _ => return,
                    };
                    if !window.is_maximized() {
                        let _ = window.drag_resize_window(direction);
                    }
                }
                _ => {}
            },
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => close_window(&can_close, control_flow),
            Event::WindowEvent {
                event: WindowEvent::Resized(_),
                ..
            } => {
                let _ = webview.evaluate_script(&format!(
                    "window.setWindowMaximized?.({})",
                    window.is_maximized()
                ));
            }
            _ => {}
        }
    });
}
fn close_window(can_close: &impl Fn() -> bool, control_flow: &mut ControlFlow) {
    if can_close() {
        *control_flow = ControlFlow::Exit;
    } else {
        rfd::MessageDialog::new()
            .set_title("Renderização em andamento")
            .set_description(
                "Aguarde a renderização terminar ou use Cancelar antes de fechar o aplicativo.",
            )
            .show();
    }
}

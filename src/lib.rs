use wasm_bindgen::prelude::*;
use web_sys::MouseEvent;

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

use dicom::object::DefaultDicomObject;
use gloo_file::Blob;
use wasm_bindgen::Clamped;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;
use web_sys::{self, CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

pub mod imaging;

use imaging::{
    byte_data_to_dicom_obj, obj_to_imagedata, update_pixel_data_lut_with, window_level_of,
    WindowLevel,
};

// When the `wee_alloc` feature is enabled, this uses `wee_alloc` as the global
// allocator.
//
// If you don't want to use `wee_alloc`, you can safely delete this.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

fn clear(context: &CanvasRenderingContext2d) -> Result<(), JsValue> {
    let width = 512;
    let height = 512;

    let mut data: Vec<u8> = (0..width * height)
        .flat_map(|_| [32, 32, 32, 255])
        .collect();

    let data = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&mut data), width, height)?;
    context.put_image_data(&data, 0., 0.)?;

    context.set_line_width(2.);
    context.set_stroke_style(&"#fff".into());

    context.begin_path();

    // Draw an outer circle.
    context.arc(255., 255., 100., 0., std::f64::consts::PI * 2.)?;

    // a vertical line
    context.move_to(255., 200.);
    context.line_to(255., 310.);

    // to the left
    context.move_to(255., 200.);
    context.line_to(210., 255.);

    // to the right
    context.move_to(255., 200.);
    context.line_to(300., 255.);

    context.stroke();

    Ok(())
}

fn set_error_messsage(msg: &str) {
    let window = web_sys::window().expect("no global `window` exists");
    let document = window.document().expect("should have a document on window");
    let error_message = document.get_element_by_id("error-message").unwrap();
    error_message.set_inner_html(msg);
}

fn render_obj_to_canvas(state: &RefCell<State>) {
    let mut state = state.borrow_mut();
    let State {
        dicom_obj,
        lut,
        window_level: _,
        canvas,
        canvas_context,
    } = &mut *state;

    let obj = if let Some(obj) = &dicom_obj {
        obj
    } else {
        gloo_console::warn!("No DICOM object loaded");
        return;
    };

    match obj_to_imagedata(obj, lut) {
        Ok(imagedata) => {
            canvas.set_width(imagedata.width());
            canvas.set_height(imagedata.height());
            canvas_context
                .put_image_data(&imagedata, 0., 0.)
                .unwrap_or_else(|e| {
                    gloo_console::error!("Error rendering image data:", e);
                });
        }
        Err(e) => {
            gloo_console::error!("Failed to render DICOM object:", e);
        }
    }
}

/// Set up the file drop zone
fn set_drop_zone(state: Rc<RefCell<State>>, element: &HtmlElement) {
    let ondrop_callback = Closure::wrap(Box::new(move |event: web_sys::DragEvent| {
        event.prevent_default();

        let data_transfer = event.data_transfer().expect("no data transfer available");
        let file_list = data_transfer.files().expect("no files available");
        let file = file_list.get(0).expect("file list is empty");

        let state = Rc::clone(&state);
        let blob: Blob = file.into();
        let file_reader = gloo_file::callbacks::read_as_bytes(&blob, move |outcome| {
            let data = outcome.expect("failed to get data");

            let dicom_obj = match byte_data_to_dicom_obj(&data) {
                Ok(obj) => obj,
                Err(e) => {
                    let error_msg = format!("Failed to parse DICOM object: {}", e);
                    gloo_console::error!(&error_msg);
                    set_error_messsage(&error_msg);
                    return;
                }
            };

            {
                let mut state = state.borrow_mut();

                // look for window level
                state.window_level = window_level_of(&dicom_obj).unwrap_or_else(|_e| None);

                state.dicom_obj = Some(dicom_obj);
                state.lut = None;

                clear(&state.out_canvas_context).unwrap();
            }

            render_obj_to_canvas(&state);
        });

        std::mem::forget(file_reader);
    }) as Box<dyn FnMut(_)>);

    let ondragover_callback = Closure::wrap(Box::new(move |event: web_sys::DragEvent| {
        event.prevent_default();
    }) as Box<dyn FnMut(_)>);

    element.set_ondragover(Some(ondragover_callback.as_ref().unchecked_ref()));
    element.set_ondrop(Some(ondrop_callback.as_ref().unchecked_ref()));

    ondrop_callback.forget();
    ondragover_callback.forget();
}

fn set_window_level_tool(state: Rc<RefCell<State>>, canvas: &HtmlCanvasElement) {
    let element = canvas;

    let is_dragging_mouse = Rc::new(Cell::new(false));

    let dragging = Rc::clone(&is_dragging_mouse);

    // on mouse down, dragging = true
    let onmousedown_callback = Closure::wrap(Box::new(move |_: MouseEvent| {
        dragging.set(true);
    }) as Box<dyn FnMut(_)>);

    // on mouse movement, update window levels if dragging
    let dragging = Rc::clone(&is_dragging_mouse);
    let onmousemove_callback = Closure::wrap(Box::new(move |ev: MouseEvent| {
        if dragging.get() {
            let ww = ev.movement_x() as f64;
            let wc = ev.movement_y() as f64 * 2.;
            change_window_level(&state, ww, wc);
        }
    }) as Box<dyn FnMut(_)>);

    // on mouse up, dragging = false
    let dragging = Rc::clone(&is_dragging_mouse);
    let onmouseup_callback = Closure::wrap(Box::new(move |_: MouseEvent| {
        dragging.set(false);
    }) as Box<dyn FnMut(_)>);

    element
        .add_event_listener_with_callback(
            "mousedown",
            onmousedown_callback.as_ref().unchecked_ref(),
        )
        .unwrap();
    element
        .add_event_listener_with_callback(
            "mousemove",
            onmousemove_callback.as_ref().unchecked_ref(),
        )
        .unwrap();
    element
        .add_event_listener_with_callback("mouseup", onmouseup_callback.as_ref().unchecked_ref())
        .unwrap();
    element
        .add_event_listener_with_callback("mouseleave", onmouseup_callback.as_ref().unchecked_ref())
        .unwrap();

    onmousedown_callback.forget();
    onmousemove_callback.forget();
    onmouseup_callback.forget();
}

fn change_window_level(state: &RefCell<State>, rel_ww: f64, rel_wc: f64) {
    {
        let mut state = state.borrow_mut();
        let State {
            dicom_obj,
            window_level,
            lut,
            ..
        } = &mut *state;

        let obj = if let Some(obj) = &dicom_obj {
            obj
        } else {
            // ignore, no DICOM object loaded
            return;
        };

        // get the current window level
        let window_level = if let Some(window_level) = window_level {
            window_level
        } else {
            // ignore, no window level available
            return;
        };

        let new_ww = (window_level.width + rel_ww).max(1.);
        let new_wc = window_level.center + rel_wc;

        // update the window level
        *window_level = WindowLevel {
            width: new_ww,
            center: new_wc,
        };
        gloo_console::debug!("[WL] updated to", new_ww, ",", new_wc);

        if let Some(lut) = lut {
            // update the LUT
            match update_pixel_data_lut_with(lut, obj, *window_level) {
                Ok(lut) => lut,
                Err(e) => {
                    gloo_console::error!("Failed to update LUT:", e);
                    return;
                }
            }
        };
    }

    // update canvas
    render_obj_to_canvas(state);
}

/// The application's global state
#[derive(Debug)]
pub struct State {
    dicom_obj: Option<DefaultDicomObject>,
    lut: Option<Vec<u8>>,
    window_level: Option<WindowLevel>,
    canvas: HtmlCanvasElement,
    canvas_context: CanvasRenderingContext2d,
}

// This is like the `main` function for our Rust webapp.
#[wasm_bindgen(start)]
pub fn main_js() -> Result<(), JsValue> {
    // This provides better error messages in debug mode.
    // It's disabled in release mode so it doesn't bloat up the file size.
    #[cfg(debug_assertions)]
    console_error_panic_hook::set_once();

    let window = web_sys::window().expect("no global `window` exists");
    let document = window.document().expect("should have a document on window");

    // fetch canvas

    let canvas = document.get_element_by_id("view").unwrap();

    let canvas: HtmlCanvasElement = canvas.dyn_into::<HtmlCanvasElement>().unwrap();

    let context = canvas
        .get_context("2d")
        .expect("Could not retrieve 2D context from canvas")
        .expect("2D context is missing")
        .dyn_into::<CanvasRenderingContext2d>()
        .unwrap();

    // clear canvas
    clear(&context).unwrap();

    // create the application state
    let state = Rc::new(RefCell::new(State {
        dicom_obj: None,
        lut: None,
        window_level: None,
        canvas: canvas.clone(),
        canvas_context: context,
    }));

    // get drop_zone
    let drop_zone = document
        .get_element_by_id("drop_zone")
        .expect("drop_zone should exist")
        .dyn_into()
        .expect("drop_zone should be an HTML element");

    set_drop_zone(Rc::clone(&state), &drop_zone);

    set_window_level_tool(Rc::clone(&state), &canvas);

    Ok(())
}

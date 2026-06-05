//! Wasm-only browser glue: wire the `#editor` textarea to a WebSocket
//! talking to the note's Durable Object. Cfg-gated to `wasm32` because it
//! depends on `web-sys` browser types; the pure `client` logic is
//! native-tested without any of this.

use std::cell::RefCell;
use std::rc::Rc;

use notes_protocol::{ClientMsg, ServerMsg};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlTextAreaElement, MessageEvent, WebSocket};

use crate::client::{ClientState, Effect};

/// Browser entrypoint. `ws_url` is the full `wss://…/note/<id>/ws`
/// endpoint; the session cookie rides the upgrade automatically (the
/// browser attaches it for same-site requests), and the server gates on
/// it. Connects, resyncs, and streams edits from the `#editor` textarea.
#[wasm_bindgen]
pub fn start(ws_url: &str) -> Result<(), JsValue> {
    let document = web_sys::window()
        .ok_or("no window")?
        .document()
        .ok_or("no document")?;
    let textarea = document
        .get_element_by_id("editor")
        .ok_or("no #editor element")?
        .dyn_into::<HtmlTextAreaElement>()?;

    let state = Rc::new(RefCell::new(ClientState::default()));
    let ws = WebSocket::new(ws_url)?;

    // onopen → resync from the last sequence we have (0 on a fresh state,
    // which still gets a full Sync back).
    {
        let ws_c = ws.clone();
        let state_c = state.clone();
        let onopen = Closure::<dyn FnMut()>::new(move || {
            let _ = send(&ws_c, &state_c.borrow().open());
        });
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();
    }

    // onmessage → fold the server frame in and adopt a Sync's body.
    {
        let textarea_c = textarea.clone();
        let state_c = state.clone();
        let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            let Some(frame) = e.data().as_string() else {
                return;
            };
            let Ok(msg) = serde_json::from_str::<ServerMsg>(&frame) else {
                return;
            };
            if let Effect::Replace(body) = state_c.borrow_mut().apply(msg) {
                textarea_c.set_value(&body);
            }
        });
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();
    }

    // input → send the whole body as an edit. The server debounces the
    // git commit, so an edit per keystroke stays cheap.
    {
        let ws_c = ws.clone();
        let state_c = state.clone();
        let textarea_c = textarea.clone();
        let oninput = Closure::<dyn FnMut()>::new(move || {
            let _ = send(&ws_c, &state_c.borrow().edit(&textarea_c.value()));
        });
        textarea.add_event_listener_with_callback("input", oninput.as_ref().unchecked_ref())?;
        oninput.forget();
    }

    // onclose → reconnect after a short delay. A fresh `start` resyncs
    // from scratch (Open{0} → full Sync), so no edits are lost.
    {
        let ws_url = ws_url.to_owned();
        let onclose = Closure::<dyn FnMut()>::new(move || {
            let url = ws_url.clone();
            let reconnect = Closure::<dyn FnMut()>::new(move || {
                let _ = start(&url);
            });
            if let Some(win) = web_sys::window() {
                let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(
                    reconnect.as_ref().unchecked_ref(),
                    1000,
                );
            }
            reconnect.forget();
        });
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();
    }

    Ok(())
}

fn send(ws: &WebSocket, msg: &ClientMsg) -> Result<(), JsValue> {
    let frame = serde_json::to_string(msg).map_err(|e| JsValue::from_str(&e.to_string()))?;
    ws.send_with_str(&frame)
}

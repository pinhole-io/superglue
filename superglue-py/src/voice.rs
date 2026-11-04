//! Realtime voice sessions for Python.
//!
//! Events reach Python on a dedicated OS thread that attaches to the
//! interpreter per event, the same hop `Client.stream` uses for tokens.
//! Commands release the GIL while they wait on the session channel, so a
//! full event queue cannot deadlock against a blocked command.

use std::future::Future;

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};
use superglue::providers::ProviderCredentials;
use superglue::realtime::{
    VoiceError, VoiceEvent, VoiceSession, voice_config_from_json, voice_event_audio,
    voice_event_json,
};

use crate::json_bridge::{json_to_py, py_to_json};
use crate::runtime;

/// A connected realtime voice session.
///
/// Create it with ``Client.voice_connect``. Methods block until the command is queued.
#[pyclass(name = "VoiceSession")]
pub(crate) struct PyVoiceSession {
    session: VoiceSession,
}

pub(crate) fn connect(
    py: Python<'_>,
    credentials: ProviderCredentials,
    config: Bound<'_, PyAny>,
    on_event: Py<PyAny>,
) -> PyResult<PyVoiceSession> {
    let config_json = py_to_json(py, config)?.to_string();
    let config = voice_config_from_json(&config_json, credentials).map_err(voice_error)?;
    let (session, mut events) = py
        .detach(|| runtime().block_on(VoiceSession::connect(config)))
        .map_err(voice_error)?;
    std::thread::Builder::new()
        .name("superglue-voice-events".to_string())
        .spawn(move || {
            while let Some(event) = events.blocking_recv() {
                Python::attach(|py| {
                    if let Err(error) = deliver(py, &on_event, &event) {
                        error.write_unraisable(py, Some(on_event.bind(py)));
                    }
                });
            }
        })
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    Ok(PyVoiceSession { session })
}

fn deliver(py: Python<'_>, on_event: &Py<PyAny>, event: &VoiceEvent) -> PyResult<()> {
    let payload = json_to_py(py, &voice_event_json(event))?;
    if let Some(audio) = voice_event_audio(event) {
        payload
            .cast::<PyDict>()?
            .set_item("audio", PyBytes::new(py, audio))?;
    }
    on_event.call1(py, (payload,))?;
    Ok(())
}

fn voice_error(error: VoiceError) -> PyErr {
    match error {
        VoiceError::Config(_) | VoiceError::UnsupportedProvider => {
            PyValueError::new_err(error.to_string())
        }
        _ => PyRuntimeError::new_err(error.to_string()),
    }
}

impl PyVoiceSession {
    fn command<F, Fut>(&self, py: Python<'_>, send: F) -> PyResult<()>
    where
        F: FnOnce(VoiceSession) -> Fut + Send,
        Fut: Future<Output = Result<(), VoiceError>>,
    {
        let session = self.session.clone();
        py.detach(move || runtime().block_on(send(session)))
            .map_err(voice_error)
    }
}

#[pymethods]
impl PyVoiceSession {
    /// Send a user text message and request a response.
    fn send_text(&self, py: Python<'_>, text: String) -> PyResult<()> {
        self.command(py, |session| async move { session.send_text(text).await })
    }

    /// Send one frame of PCM16 mono audio at the session sample rate.
    fn send_audio(&self, py: Python<'_>, audio: Bound<'_, PyBytes>) -> PyResult<()> {
        let audio = audio.as_bytes().to_vec();
        self.command(py, |session| async move { session.send_audio(audio).await })
    }

    /// Stop the current response.
    fn interrupt(&self, py: Python<'_>) -> PyResult<()> {
        self.command(py, |session| async move { session.interrupt().await })
    }

    /// Return a tool result for a ``function_call`` event and request the next response.
    ///
    /// ``output`` must be JSON-serializable.
    fn submit_tool_result(
        &self,
        py: Python<'_>,
        call_id: String,
        output: Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let output = py_to_json(py, output)?;
        self.command(py, |session| async move {
            session.submit_tool_result(call_id, output).await?;
            session.create_response().await
        })
    }

    /// Close the provider connection. A closed session rejects later commands.
    fn close(&self, py: Python<'_>) -> PyResult<()> {
        self.command(py, |session| async move { session.close().await })
    }
}

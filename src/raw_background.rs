//! One owned reconstruction worker; superseded requests cancel at tile boundaries.
use crate::{input::SensorImage, ml_runtime::InferenceDevice, raw_ml::BayerModel};
use anyhow::{Context, Result};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};

pub(crate) struct Job {
    pub source: Arc<SensorImage>,
    pub hot_pixels: bool,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
    result: mpsc::Receiver<Result<Arc<SensorImage>, String>>,
}
impl Job {
    pub fn matches(&self, source: &Arc<SensorImage>, hot: bool) -> bool {
        Arc::ptr_eq(&self.source, source) && self.hot_pixels == hot
    }
    pub fn progress(&self) -> [usize; 2] {
        [
            self.done.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
        ]
    }
    pub fn poll(&self) -> Result<Option<Arc<SensorImage>>> {
        match self.result.try_recv() {
            Ok(Ok(image)) => Ok(Some(image)),
            Ok(Err(error)) => anyhow::bail!(error),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => anyhow::bail!("Reconstruction worker stopped"),
        }
    }
    pub fn wait(&self) -> Result<Arc<SensorImage>> {
        self.result
            .recv()
            .context("Reconstruction worker stopped")?
            .map_err(anyhow::Error::msg)
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

struct Request {
    source: Arc<SensorImage>,
    hot: bool,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
    result: mpsc::Sender<Result<Arc<SensorImage>, String>>,
}

pub(crate) struct Service {
    requests: mpsc::Sender<Request>,
}
impl Service {
    pub fn open(model_directory: PathBuf) -> Result<Self> {
        let mut model = None;
        Self::spawn(move |request| {
            if model.is_none() {
                model = Some(BayerModel::open(&model_directory, InferenceDevice::Auto)?);
            }
            let image = model.as_ref().unwrap().reconstruct_image_controlled(
                &request.source,
                request.hot,
                1024,
                |done, total| {
                    request.done.store(done, Ordering::Relaxed);
                    request.total.store(total, Ordering::Relaxed);
                    !request.cancel.load(Ordering::Acquire)
                },
            )?;
            Ok(Arc::new(image))
        })
    }
    fn spawn(
        mut process: impl FnMut(&Request) -> Result<Arc<SensorImage>> + Send + 'static,
    ) -> Result<Self> {
        let (requests, receiver) = mpsc::channel::<Request>();
        thread::Builder::new()
            .name("rawpuppy-reconstruction".into())
            .spawn(move || {
                while let Ok(mut request) = receiver.recv() {
                    // All superseded jobs are already cancelled by their owner.
                    while let Ok(next) = receiver.try_recv() {
                        request = next;
                    }
                    if request.cancel.load(Ordering::Acquire) {
                        continue;
                    }
                    let result = process(&request);
                    let _ = request.result.send(result.map_err(|e| format!("{e:#}")));
                }
            })?;
        Ok(Self { requests })
    }
    pub fn request(&self, source: Arc<SensorImage>, hot_pixels: bool) -> Result<Job> {
        let cancel = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicUsize::new(0));
        let total = Arc::new(AtomicUsize::new(0));
        let (result, receiver) = mpsc::channel();
        self.requests
            .send(Request {
                source: source.clone(),
                hot: hot_pixels,
                cancel: cancel.clone(),
                done: done.clone(),
                total: total.clone(),
                result,
            })
            .context("Reconstruction worker stopped")?;
        Ok(Job {
            source,
            hot_pixels,
            cancel,
            done,
            total,
            result: receiver,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    fn image(name: &str) -> Arc<SensorImage> {
        let mut image = SensorImage::from_rgb(2, 2, vec![0.2; 12]).unwrap();
        image.metadata.model = name.into();
        Arc::new(image)
    }
    #[test]
    fn cancelled_and_superseded_requests_release_sources_and_only_latest_runs() {
        let (started, events) = mpsc::channel();
        let service = Service::spawn(move |request| {
            started.send(request.source.metadata.model.clone()).unwrap();
            if request.source.metadata.model == "first" {
                while !request.cancel.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(1));
                }
                anyhow::bail!("Cancelled test request");
            }
            Ok(request.source.clone())
        })
        .unwrap();
        let first_source = image("first");
        let retired = Arc::downgrade(&first_source);
        let first = service.request(first_source.clone(), false).unwrap();
        assert_eq!(
            events.recv_timeout(Duration::from_secs(2)).unwrap(),
            "first"
        );
        drop(first_source);
        let middle = service.request(image("middle"), false).unwrap();
        drop(middle);
        let latest_source = image("latest");
        let latest = service.request(latest_source.clone(), true).unwrap();
        drop(first);
        assert_eq!(
            events.recv_timeout(Duration::from_secs(2)).unwrap(),
            "latest"
        );
        assert!(Arc::ptr_eq(&latest.wait().unwrap(), &latest_source));
        assert!(retired.upgrade().is_none());
        assert!(events.try_recv().is_err());
    }
}

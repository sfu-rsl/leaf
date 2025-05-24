use core::borrow::Borrow;

use super::{
    CurrentSolverCase, CurrentSolverValue, IStep, Step,
    backend::{ConstValue, ValueRef},
};

#[derive(Debug)]
pub(super) struct Translation<V, T>(V, T);

impl<V, T> Translation<V, T> {
    pub fn of(value: V, translator: impl FnOnce(&V) -> T) -> Self {
        let translated = translator(&value);
        Self(value, translated)
    }
}

impl<T> Borrow<ValueRef> for Translation<ValueRef, T> {
    fn borrow(&self) -> &ValueRef {
        &self.0
    }
}

impl<T> Borrow<ConstValue> for Translation<ConstValue, T> {
    fn borrow(&self) -> &ConstValue {
        &self.0
    }
}

impl<'ctx, V> Borrow<CurrentSolverValue<'ctx>> for Translation<V, CurrentSolverValue<'ctx>> {
    fn borrow(&self) -> &CurrentSolverValue<'ctx> {
        &self.1
    }
}

impl<'ctx, V> Borrow<CurrentSolverCase<'ctx>> for Translation<V, CurrentSolverCase<'ctx>> {
    fn borrow(&self) -> &CurrentSolverCase<'ctx> {
        &self.1
    }
}

impl Borrow<Step> for IStep {
    fn borrow(&self) -> &Step {
        &self.value.borrow()
    }
}

pub(super) mod dumping {
    use std::fs;

    use common::log_debug;
    use serde::de::DeserializeOwned;

    use crate::{
        abs::Constraint,
        trace::{FilterStepInspectorExt, StepInspector},
        utils::{
            alias::RRef,
            file::{FileFormat, FileGenConfig},
        },
    };

    pub(crate) trait Dumper {
        fn dump(&mut self) -> Result<(), String>;
    }

    impl Dumper for Box<dyn Dumper + '_> {
        fn dump(&mut self) -> Result<(), String> {
            self.as_mut().dump()
        }
    }

    impl<F: FnMut() -> Result<(), String>> Dumper for F {
        fn dump(&mut self) -> Result<(), String> {
            self.call_mut(())
        }
    }

    impl<D: Dumper> Dumper for Vec<D> {
        fn dump(&mut self) -> Result<(), String> {
            for dumper in self.iter_mut() {
                dumper.dump()?
            }
            Ok(())
        }
    }

    pub(crate) trait DumperListExt {
        fn extend_opt<'a>(&mut self, dumper: Option<impl Dumper + 'a>)
        where
            Self: Extend<Box<dyn Dumper + 'a>>;
    }
    impl<'b> DumperListExt for Vec<Box<dyn Dumper + 'b>> {
        fn extend_opt<'a>(&mut self, dumper: Option<impl Dumper + 'a>)
        where
            Self: Extend<Box<dyn Dumper + 'a>>,
        {
            Extend::extend(
                self,
                dumper
                    .into_iter()
                    .map(Box::new)
                    .map(|d| d as Box<dyn Dumper>),
            )
        }
    }

    macro_rules! create_ser_dumper {
        ($config: expr, $name: expr, $default_filename: expr, || {$data: expr}) => {{
            use std::io::{Seek, Write};

            use crate::utils::file::{FileFormat, FileGenConfig};

            let create_serializer = |config: &FileGenConfig,
                                     name: String,
                                     default_filename: &str| {
                let mut file = config
                    .open_or_create_single(&default_filename, false)
                    .unwrap_or_else(|e| panic!("Could not create file for {name}: {e}"));

                match config.format {
                    FileFormat::Json => {
                        // FIXME: Introduce a naive journaling to prevent corruption
                        let mut serializer = serde_json::Serializer::new(file.try_clone().unwrap());
                        Box::new(move || {
                            file.rewind().and_then(|_| file.set_len(0)).map_err(|e| {
                                format!("{}: Could not truncate file: {}", name, e.to_string())
                            })?;
                            serde::Serialize::serialize(&$data, &mut serializer)
                                .map(|_| ())
                                .map_err(|e| format!("{}: {}", name, e.to_string()))?;
                            file.flush().map_err(|e| {
                                format!("{}: Could not flush file: {}", name, e.to_string(),)
                            })?;
                            Ok(())
                        })
                    }
                    FileFormat::JsonLines => {
                        unimplemented!("Json stream format is not supported for this dumper")
                    }
                    FileFormat::Binary => {
                        unimplemented!("Binary output format is not supported for this dumper")
                    }
                }
            };

            create_serializer($config, $name, $default_filename)
        }};
    }
    pub(crate) use create_ser_dumper;

    pub(crate) fn deserialize_snapshot<T: DeserializeOwned + Default>(
        config: &FileGenConfig,
        default_filename: &str,
    ) -> Option<Result<T, String>> {
        use std::io::Seek;

        let file_path = config.single_file_path(&default_filename);
        if !file_path.exists() {
            return None;
        }

        let deserialize = || -> Result<T, String> {
            let mut file = fs::File::open(&file_path).map_err(|e| {
                format!(
                    "Problem in opening the file `{}`:{}",
                    file_path.display(),
                    e.to_string()
                )
            })?;

            if file.stream_len().is_ok_and(|l| l == 0) {
                return Ok(T::default());
            }

            let snapshot = match config.format {
                FileFormat::Json => serde_json::from_reader(file)
                    .map_err(|e| format!("Problem in parsing the file: {}", e.to_string())),
                _ => {
                    unimplemented!("Only json is expected for snapshot")
                }
            };
            snapshot
        };

        Some(deserialize())
    }

    pub(crate) fn dump(dumper: &RRef<impl Dumper>) {
        log_debug!("Dumped trace managing data");
        dumper
            .as_ref()
            .borrow_mut()
            .dump()
            .expect("Problem with dumping information")
    }

    pub(crate) fn create_timer_dumper_inspector<'a, S: 'a, V: 'a, C: 'a>(
        dumper: RRef<impl Dumper + 'a>,
        interval: Option<core::time::Duration>,
    ) -> Box<dyn StepInspector<S, V, C> + 'a> {
        if let Some(interval) = interval {
            Box::new(create_dumper_inspector(dumper).timer_freq_filtered(interval))
        } else {
            Box::new(()) // NoopInspector
        }
    }

    fn create_dumper_inspector<'a, S, V, C>(
        dumper: RRef<impl Dumper + 'a>,
    ) -> impl StepInspector<S, V, C> {
        move |_: &S, _: Constraint<&V, &C>| dump(&dumper)
    }
}

mod shutdown {
    use delegate::delegate;

    use crate::abs::backend::{Shutdown, TraceManager as AbsTraceManager};

    pub(super) struct ShutdownWrapper<T, F> {
        inner: T,
        f: F,
    }

    impl<T, F> ShutdownWrapper<T, F> {
        pub(super) fn new(inner: T, f: F) -> Self {
            Self { inner, f }
        }
    }

    impl<T, F> Shutdown for ShutdownWrapper<T, F>
    where
        F: FnMut(),
    {
        fn shutdown(&mut self) {
            (self.f)();
        }
    }

    impl<T, F, S, V, C> AbsTraceManager<S, V, C> for ShutdownWrapper<T, F>
    where
        T: AbsTraceManager<S, V, C>,
    {
        delegate! {
            to self.inner {
                fn notify_step(&mut self, step: S, constraint: crate::abs::Constraint<V, C>);
            }
        }
    }

    pub(crate) trait TraceManagerExt<S, V, C>: AbsTraceManager<S, V, C> {
        fn on_shutdown(self, f: impl FnMut()) -> impl AbsTraceManager<S, V, C> + Shutdown;
    }
    impl<S, V, C, M: AbsTraceManager<S, V, C>> TraceManagerExt<S, V, C> for M {
        fn on_shutdown(self, f: impl FnMut()) -> impl AbsTraceManager<S, V, C> + Shutdown {
            ShutdownWrapper::new(self, f)
        }
    }
}

pub(super) use shutdown::TraceManagerExt as ShutdownTraceManagerExt;

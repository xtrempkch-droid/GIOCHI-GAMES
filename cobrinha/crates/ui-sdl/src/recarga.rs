//! Hot reload da engine 0.5: o shader da fruta e o OBJ são recompilados/
//! recarregados no mesmo handle quando o arquivo muda.
//!
//! O vigia é opt-in fora de debug (`GIOCHI_HOT_RELOAD=1`), porque um inotify por
//! jogo num aparelho que só quer jogar é custo sem retorno.

use std::path::PathBuf;

use giochi_engine_3d::render3d::{MeshHandle, ShaderHandle};
use giochi_engine_3d::RenderContext3D;

#[cfg(feature = "hot_reload")]
mod com_recarga {
    use super::*;
    use giochi_engine::hot_reload::{FileWatcher, HotReloadConfig};
    use giochi_engine_3d::hot_reload::ShaderReloader;

    pub struct Recarga {
        watcher: Option<FileWatcher>,
        reloader: ShaderReloader,
        habilitado: bool,
    }

    impl Recarga {
        pub fn novo(
            shader: Option<(PathBuf, PathBuf, ShaderHandle)>,
            modelo: Option<(PathBuf, MeshHandle)>,
        ) -> Self {
            let config = HotReloadConfig::from_env();
            if !config.enabled {
                return Self {
                    watcher: None,
                    reloader: ShaderReloader::new(),
                    habilitado: false,
                };
            }

            let mut reloader = ShaderReloader::new();
            let mut arquivos = Vec::new();
            if let Some((vert, frag, handle)) = shader {
                reloader.track_shader(handle, &vert, &frag);
                arquivos.push(vert);
                arquivos.push(frag);
            }
            if let Some((caminho, handle)) = modelo {
                reloader.track_mesh(handle, &caminho);
                arquivos.push(caminho);
            }
            if arquivos.is_empty() {
                return Self {
                    watcher: None,
                    reloader,
                    habilitado: false,
                };
            }

            let mut watcher = FileWatcher::with_debounce(config.debounce());
            match watcher.watch_files(&arquivos) {
                Ok(_) => Self {
                    watcher: Some(watcher),
                    reloader,
                    habilitado: true,
                },
                Err(erro) => {
                    eprintln!("cobrinha: hot reload indisponível ({erro})");
                    Self {
                        watcher: None,
                        reloader,
                        habilitado: false,
                    }
                }
            }
        }

        pub fn atualizar(&mut self, render: &mut RenderContext3D) -> usize {
            if !self.habilitado {
                return 0;
            }
            let Some(watcher) = self.watcher.as_mut() else {
                return 0;
            };
            let mut aplicados = 0;
            for evento in watcher.poll() {
                if evento.is_deleted() {
                    continue;
                }
                aplicados += self.reloader.apply_path(render, evento.path());
            }
            if aplicados > 0 {
                eprintln!("cobrinha: hot reload — {aplicados} recurso(s) recarregado(s)");
            }
            aplicados
        }

        pub fn resumo(&self) -> String {
            if !self.habilitado {
                return "hot reload off".to_owned();
            }
            let stats = self.reloader.stats();
            format!(
                "hot reload | shader {}|{} | malha {}|{}",
                stats.shaders_reloaded,
                stats.shader_failures,
                stats.meshes_reloaded,
                stats.mesh_failures
            )
        }

        pub fn ativo(&self) -> bool {
            self.habilitado
        }
    }
}

#[cfg(not(feature = "hot_reload"))]
mod sem_recarga {
    use super::*;

    pub struct Recarga;

    impl Recarga {
        pub fn novo(
            _shader: Option<(PathBuf, PathBuf, ShaderHandle)>,
            _modelo: Option<(PathBuf, MeshHandle)>,
        ) -> Self {
            Self
        }

        pub fn atualizar(&mut self, _render: &mut RenderContext3D) -> usize {
            0
        }

        pub fn resumo(&self) -> String {
            "hot reload fora desta build".to_owned()
        }

        pub fn ativo(&self) -> bool {
            false
        }
    }
}

#[cfg(feature = "hot_reload")]
pub use com_recarga::Recarga;
#[cfg(not(feature = "hot_reload"))]
pub use sem_recarga::Recarga;

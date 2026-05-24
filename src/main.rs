slint::include_modules!();
use arboard::Clipboard;
use chrono::{Datelike, Local};
use serde::{Deserialize, Serialize};
use slint::{Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

#[derive(Serialize, Deserialize)]
struct Config {
    directorio_base: Option<String>,
}

fn ruta_config() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
        .join("notas-semanales")
        .join("config.toml")
}

fn cargar_config() -> Config {
    let ruta = ruta_config();
    fs::read_to_string(&ruta)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or(Config {
            directorio_base: None,
        })
}

#[allow(dead_code)]
fn guardar_config(cfg: &Config) {
    if let Ok(contenido) = toml::to_string(cfg) {
        let ruta = ruta_config();
        if let Some(padre) = ruta.parent() {
            let _ = fs::create_dir_all(padre);
        }
        let _ = fs::write(ruta, contenido);
    }
}

const MAX_HISTORIAL: usize = 50;

struct EstadoEditor {
    archivo_activo: String,
    lineas: Vec<String>,
    linea_activa: usize,
    historial: Vec<Vec<String>>,
    historial_pos: usize,
}

impl EstadoEditor {
    fn guardar_snapshot(&mut self) {
        self.historial.truncate(self.historial_pos);
        self.historial.push(self.lineas.clone());
        if self.historial.len() > MAX_HISTORIAL {
            self.historial.remove(0);
        } else {
            self.historial_pos += 1;
        }
    }

    fn undo(&mut self) -> bool {
        if self.historial_pos < 2 {
            return false;
        }
        self.historial_pos -= 1;
        self.lineas = self.historial[self.historial_pos - 1].clone();
        true
    }

    fn redo(&mut self) -> bool {
        if self.historial_pos >= self.historial.len() {
            return false;
        }
        self.lineas = self.historial[self.historial_pos].clone();
        self.historial_pos += 1;
        true
    }
}

fn obtener_directorio_base() -> PathBuf {
    let cfg = cargar_config();
    if let Some(dir) = cfg.directorio_base {
        return PathBuf::from(dir);
    }
    dirs::document_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
        .expect("No se encontró directorio de usuario")
        .join("Boveda_Semanales")
}

fn obtener_nombre_semana_actual() -> String {
    let fecha = Local::now();
    format!("{}-W{:02}.md", fecha.year(), fecha.iso_week().week())
}

fn parsear_año_semana(nombre_archivo: &str) -> Option<(i32, u32)> {
    // Formato esperado: YYYY-Www.md
    let nombre = nombre_archivo.strip_suffix(".md")?;
    let (año_str, semana_str) = nombre.split_once("-W")?;
    let año = año_str.parse::<i32>().ok()?;
    let semana = semana_str.parse::<u32>().ok()?;
    Some((año, semana))
}

fn puede_crear_semana_actual(notas: &[String], nombre_semana_actual: &str) -> bool {
    // Regla: se puede crear la nota si la semana actual es distinta a la última semana creada.
    // (Si no hay notas, se permite crear.)
    let actual = match parsear_año_semana(nombre_semana_actual) {
        Some(v) => v,
        None => return true,
    };

    let ultima = notas.iter().filter_map(|n| parsear_año_semana(n)).max();

    match ultima {
        None => true,
        Some(u) => u != actual,
    }
}

fn recolectar_tareas_pendientes(directorio_base: &Path) -> Vec<String> {
    let nombre_actual = obtener_nombre_semana_actual();
    let semana_actual = parsear_año_semana(&nombre_actual);
    let mut resultado = Vec::new();
    let notas = leer_lista_notas(directorio_base);
    for nombre in &notas {
        if let Some(semana_nota) = parsear_año_semana(nombre) {
            if Some(semana_nota) >= semana_actual {
                continue;
            }
        }
        let lineas = cargar_archivo_fisico(nombre);
        for linea in lineas {
            if let Some(tarea) = linea.strip_prefix("- [ ] ") {
                resultado.push(format!("[{}] {}", nombre.replace(".md", ""), tarea));
            }
        }
    }
    resultado
}

fn contar_tareas(nombre_archivo: &str) -> (u32, u32) {
    let lineas = cargar_archivo_fisico(nombre_archivo);
    let total = lineas
        .iter()
        .filter(|l| l.starts_with("- [ ] ") || l.starts_with("- [x] ") || l.starts_with("- [X] "))
        .count() as u32;
    let completadas = lineas
        .iter()
        .filter(|l| l.starts_with("- [x] ") || l.starts_with("- [X] "))
        .count() as u32;
    (completadas, total)
}

fn leer_lista_notas(directorio_base: &Path) -> Vec<String> {
    let mut notas = Vec::new();
    if let Ok(entradas) = fs::read_dir(directorio_base) {
        for entrada in entradas.flatten() {
            if entrada.path().is_dir() {
                if let Ok(archivos) = fs::read_dir(entrada.path()) {
                    for archivo in archivos.flatten() {
                        if let Some(nombre) = archivo.file_name().to_str() {
                            if nombre.ends_with(".md") {
                                notas.push(nombre.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    notas.sort_by(|a, b| b.cmp(a)); // Más recientes arriba
    notas
}

fn guardar_archivo_fisico(nombre_archivo: &str, lineas: &[String]) {
    if nombre_archivo.is_empty() {
        return;
    }
    let año = nombre_archivo.split('-').next().unwrap_or("");
    let mut ruta = obtener_directorio_base();
    ruta.push(año);
    if !ruta.exists() {
        if let Err(e) = fs::create_dir_all(&ruta) {
            eprintln!("Error creando directorio {:?}: {}", ruta, e);
            return;
        }
    }
    ruta.push(nombre_archivo);
    fs::write(ruta, lineas.join("\n")).ok();
}

fn eliminar_archivo_fisico(nombre_archivo: &str) -> bool {
    if nombre_archivo.is_empty() {
        return false;
    }
    let año = nombre_archivo.split('-').next().unwrap_or("");
    let mut ruta = obtener_directorio_base();
    ruta.push(año);
    ruta.push(nombre_archivo);
    fs::remove_file(ruta).is_ok()
}

fn cargar_template(nombre_semana: &str) -> Vec<String> {
    let ruta_template = obtener_directorio_base().join("template.md");
    if let Ok(contenido) = fs::read_to_string(&ruta_template) {
        let semana = nombre_semana.replace(".md", "");
        return contenido
            .replace("{{semana}}", &semana)
            .lines()
            .map(|s| s.to_string())
            .collect();
    }
    vec![
        format!("# Planificación / {}", nombre_semana.replace(".md", "")),
        "## Tareas".into(),
        "- [ ] ".into(),
    ]
}

fn cargar_archivo_fisico(nombre_archivo: &str) -> Vec<String> {
    let año = nombre_archivo.split('-').next().unwrap_or("");
    let mut ruta = obtener_directorio_base();
    ruta.push(año);
    ruta.push(nombre_archivo);
    fs::read_to_string(ruta)
        .map(|c| c.lines().map(|s| s.to_string()).collect())
        .unwrap_or_default()
}

fn determinar_tipo(texto: &str) -> String {
    if texto.starts_with("### ") {
        "titulo3".to_string()
    } else if texto.starts_with("## ") {
        "titulo2".to_string()
    } else if texto.starts_with("# ") {
        "titulo1".to_string()
    } else if texto.starts_with("- [ ] ") {
        "tarea_pendiente".to_string()
    } else if texto.starts_with("- [x] ") || texto.starts_with("- [X] ") {
        "tarea_completada".to_string()
    } else if texto.starts_with("- ") || texto.starts_with("* ") {
        "vinieta".to_string()
    } else {
        "parrafo".to_string()
    }
}

fn reconstruir_modelo(estado: &EstadoEditor) -> Rc<VecModel<LineaNota>> {
    let modelo = Rc::new(VecModel::default());
    for (i, linea_str) in estado.lineas.iter().enumerate() {
        let tipo = determinar_tipo(linea_str);
        let texto_limpio = match tipo.as_str() {
            "titulo1" => linea_str[2..].to_string(),
            "titulo2" => linea_str[3..].to_string(),
            "titulo3" => linea_str[4..].to_string(),
            "tarea_pendiente" => linea_str.replacen("- [ ] ", "", 1),
            "tarea_completada" => linea_str
                .replacen("- [x] ", "", 1)
                .replacen("- [X] ", "", 1),
            "vinieta" => linea_str[2..].to_string(),
            _ => linea_str.to_string(),
        };
        modelo.push(LineaNota {
            texto_crudo: SharedString::from(linea_str),
            texto_limpio: SharedString::from(texto_limpio),
            es_activa: i == estado.linea_activa,
            tipo_bloque: SharedString::from(tipo),
        });
    }
    modelo
}

fn main() -> Result<(), slint::PlatformError> {
    let directorio_base = obtener_directorio_base();
    if !directorio_base.exists() {
        if let Err(e) = fs::create_dir_all(&directorio_base) {
            eprintln!("Error creando directorio base {:?}: {}", directorio_base, e);
        }
    }

    let ui = AppWindow::new()?;
    let estado = Rc::new(RefCell::new(EstadoEditor {
        archivo_activo: String::new(),
        lineas: Vec::new(),
        linea_activa: 999,
        historial: Vec::new(),
        historial_pos: 0,
    }));

    let nombre_actual = obtener_nombre_semana_actual();
    let lista_archivos = leer_lista_notas(&directorio_base);
    ui.set_puede_crear_nota(puede_crear_semana_actual(&lista_archivos, &nombre_actual));
    ui.set_nota_activa(SharedString::from(""));
    ui.set_lineas_documento(ModelRc::from(Rc::new(VecModel::<LineaNota>::default())));

    let modelo_nombres = Rc::new(VecModel::from(
        lista_archivos
            .iter()
            .map(|nombre| {
                let (comp, total) = contar_tareas(nombre);
                EntradaNota {
                    nombre: SharedString::from(nombre),
                    resumen: SharedString::from(if total == 0 {
                        String::new()
                    } else {
                        format!("{}/{} tareas", comp, total)
                    }),
                }
            })
            .collect::<Vec<_>>(),
    ));
    ui.set_lista_notas(ModelRc::from(modelo_nombres.clone()));

    let tareas_pendientes = recolectar_tareas_pendientes(&directorio_base);
    let modelo_resumen = Rc::new(VecModel::from(
        tareas_pendientes
            .iter()
            .map(SharedString::from)
            .collect::<Vec<_>>(),
    ));
    ui.set_resumen_tareas(ModelRc::from(modelo_resumen.clone()));

    // Closure que reconstruye el resumen leyendo del disco; se captura en los callbacks
    let dir_resumen = directorio_base.clone();
    let modelo_resumen_ref = modelo_resumen.clone();
    let refrescar_resumen = Rc::new(move || {
        modelo_resumen_ref.set_vec(
            recolectar_tareas_pendientes(&dir_resumen)
                .into_iter()
                .map(SharedString::from)
                .collect::<Vec<_>>(),
        );
    });

    // Lista completa para restaurar búsqueda
    let todas_las_notas: Rc<RefCell<Vec<EntradaNota>>> = Rc::new(RefCell::new(
        lista_archivos
            .iter()
            .map(|nombre| {
                let (comp, total) = contar_tareas(nombre);
                EntradaNota {
                    nombre: SharedString::from(nombre),
                    resumen: SharedString::from(if total == 0 {
                        String::new()
                    } else {
                        format!("{}/{} tareas", comp, total)
                    }),
                }
            })
            .collect(),
    ));

    let modelo_busqueda = modelo_nombres.clone();
    let todas_c = todas_las_notas.clone();
    ui.on_buscar(move |query| {
        let q = query.to_lowercase();
        let todas = todas_c.borrow();
        let filtradas: Vec<EntradaNota> = if q.is_empty() {
            todas.clone()
        } else {
            todas
                .iter()
                .filter(|e| e.nombre.to_lowercase().contains(&q))
                .cloned()
                .collect()
        };
        while modelo_busqueda.row_count() > 0 {
            modelo_busqueda.remove(0);
        }
        for entrada in filtradas {
            modelo_busqueda.push(entrada);
        }
    });

    // --- CALLBACKS ---
    let ui_handle = ui.as_weak();
    let est_c = estado.clone();
    ui.on_nota_seleccionada(move |n| {
        let mut est = est_c.borrow_mut();
        est.archivo_activo = n.to_string();
        est.lineas = cargar_archivo_fisico(n.as_str());
        est.linea_activa = 999;
        est.historial.clear();
        est.historial_pos = 0;
        est.guardar_snapshot();
        if let Some(ui) = ui_handle.upgrade() {
            ui.set_nota_activa(n.clone());
            ui.set_lineas_documento(ModelRc::from(reconstruir_modelo(&est)));
        }
    });

    let modelo_nombres_crear = modelo_nombres.clone();
    let ui_handle = ui.as_weak();
    let est_c = estado.clone();
    ui.on_crear_nota(move || {
        let n = obtener_nombre_semana_actual();
        let p = cargar_template(&n);
        guardar_archivo_fisico(&n, &p);
        let mut est = est_c.borrow_mut();
        est.archivo_activo = n.clone();
        est.lineas = p;
        est.linea_activa = 2;
        est.historial.clear();
        est.historial_pos = 0;
        est.guardar_snapshot();
        if let Some(ui) = ui_handle.upgrade() {
            ui.set_nota_activa(SharedString::from(&n));
            ui.set_lineas_documento(ModelRc::from(reconstruir_modelo(&est)));
            ui.set_puede_crear_nota(false);
            modelo_nombres_crear.insert(
                0,
                EntradaNota {
                    nombre: SharedString::from(&n),
                    resumen: SharedString::from("0/1 tareas"),
                },
            );
        }
    });

    let save_timer = Rc::new(RefCell::new(Timer::default()));
    let est_c = estado.clone();
    let save_timer_c = save_timer.clone();
    let refrescar_actualizar = refrescar_resumen.clone();
    ui.on_actualizar_linea(move |idx, txt| {
        let mut est = est_c.borrow_mut();
        if let Some(l) = est.lineas.get_mut(idx as usize) {
            *l = txt.to_string();
        }
        let archivo = est.archivo_activo.clone();
        let lineas = est.lineas.clone();
        drop(est);
        let refrescar = refrescar_actualizar.clone();
        save_timer_c.borrow_mut().start(
            TimerMode::SingleShot,
            Duration::from_millis(500),
            move || {
                guardar_archivo_fisico(&archivo, &lineas);
                refrescar();
            },
        );
    });

    // Toggle de checklist desde modo lectura (CheckBox).
    let ui_handle = ui.as_weak();
    let est_c = estado.clone();
    let modelo_nombres_toggle = modelo_nombres.clone();
    let refrescar_toggle = refrescar_resumen.clone();
    ui.on_toggle_tarea(move |idx, marcada| {
        let mut est = est_c.borrow_mut();
        let i = idx as usize;
        if i >= est.lineas.len() {
            return;
        }

        let linea = est.lineas[i].clone();
        let actual = linea.trim_end_matches('\r');
        let nueva = if marcada {
            if actual.starts_with("- [ ] ") {
                actual.replacen("- [ ] ", "- [x] ", 1)
            } else if actual.starts_with("- [x] ") || actual.starts_with("- [X] ") {
                actual.to_string()
            } else {
                // Si no es una tarea, no hacemos nada.
                actual.to_string()
            }
        } else {
            if actual.starts_with("- [x] ") || actual.starts_with("- [X] ") {
                actual
                    .replacen("- [x] ", "- [ ] ", 1)
                    .replacen("- [X] ", "- [ ] ", 1)
            } else {
                actual.to_string()
            }
        };

        if nueva != actual {
            est.guardar_snapshot();
            est.lineas[i] = nueva;
            guardar_archivo_fisico(&est.archivo_activo, &est.lineas);
            let archivo = est.archivo_activo.clone();
            let (comp, total) = {
                let comp = est
                    .lineas
                    .iter()
                    .filter(|l| l.starts_with("- [x] ") || l.starts_with("- [X] "))
                    .count() as u32;
                let total = est
                    .lineas
                    .iter()
                    .filter(|l| {
                        l.starts_with("- [ ] ")
                            || l.starts_with("- [x] ")
                            || l.starts_with("- [X] ")
                    })
                    .count() as u32;
                (comp, total)
            };
            if let Some(pos) = (0..modelo_nombres_toggle.row_count()).find(|&j| {
                modelo_nombres_toggle
                    .row_data(j)
                    .map(|e| e.nombre.as_str().to_string())
                    == Some(archivo.clone())
            }) {
                modelo_nombres_toggle.set_row_data(
                    pos,
                    EntradaNota {
                        nombre: SharedString::from(&archivo),
                        resumen: SharedString::from(format!("{}/{} tareas", comp, total)),
                    },
                );
            }
            refrescar_toggle();
            if let Some(ui) = ui_handle.upgrade() {
                ui.set_lineas_documento(ModelRc::from(reconstruir_modelo(&est)));
            }
        }
    });

    let ui_handle = ui.as_weak();
    let est_c = estado.clone();
    ui.on_cambiar_foco(move |idx| {
        let mut est = est_c.borrow_mut();
        est.linea_activa = idx as usize;
        if let Some(ui) = ui_handle.upgrade() {
            ui.set_lineas_documento(ModelRc::from(reconstruir_modelo(&est)));
        }
    });

    // Enter: inserta una nueva línea debajo y mueve el foco.
    let ui_handle = ui.as_weak();
    let est_c = estado.clone();
    let refrescar_insertar = refrescar_resumen.clone();
    ui.on_insertar_linea(move |idx, txt| {
        let mut est = est_c.borrow_mut();
        if est.archivo_activo.is_empty() {
            return;
        }

        let i = idx as usize;
        if i >= est.lineas.len() {
            return;
        }

        est.guardar_snapshot();
        est.lineas[i] = txt.to_string();
        let tipo = determinar_tipo(&est.lineas[i]);
        let es_prefijo_vacio = matches!(
            (tipo.as_str(), est.lineas[i].as_str()),
            ("tarea_pendiente", "- [ ] ")
                | ("tarea_completada", "- [x] ")
                | ("tarea_completada", "- [X] ")
                | ("vinieta", "- ")
                | ("vinieta", "* ")
        );
        if es_prefijo_vacio {
            est.lineas[i] = String::new();
            // linea_activa se mantiene en i: sale del modo lista
        } else {
            let nueva = match tipo.as_str() {
                "tarea_pendiente" | "tarea_completada" => "- [ ] ".to_string(),
                "vinieta" => {
                    if est.lineas[i].starts_with("* ") {
                        "* ".to_string()
                    } else {
                        "- ".to_string()
                    }
                }
                _ => String::new(),
            };
            est.lineas.insert(i + 1, nueva);
            est.linea_activa = i + 1;
        }
        guardar_archivo_fisico(&est.archivo_activo, &est.lineas);
        refrescar_insertar();

        if let Some(ui) = ui_handle.upgrade() {
            ui.set_lineas_documento(ModelRc::from(reconstruir_modelo(&est)));
        }
    });

    // Flechas arriba/abajo: mueve el foco entre líneas.
    let ui_handle = ui.as_weak();
    let est_c = estado.clone();
    ui.on_mover_foco(move |delta| {
        let mut est = est_c.borrow_mut();
        if est.archivo_activo.is_empty() {
            return;
        }
        if est.lineas.is_empty() {
            return;
        }

        let cur = if est.linea_activa >= est.lineas.len() {
            0
        } else {
            est.linea_activa
        };
        let next = if delta < 0 {
            cur.saturating_sub(1)
        } else if delta > 0 {
            (cur + 1).min(est.lineas.len().saturating_sub(1))
        } else {
            cur
        };

        est.linea_activa = next;
        if let Some(ui) = ui_handle.upgrade() {
            ui.set_lineas_documento(ModelRc::from(reconstruir_modelo(&est)));
        }
    });

    let ui_handle = ui.as_weak();
    let est_c = estado.clone();
    let refrescar_eliminar = refrescar_resumen.clone();
    ui.on_eliminar_linea(move |idx| {
        let mut est = est_c.borrow_mut();
        let i = idx as usize;
        if i == 0 || i >= est.lineas.len() {
            return;
        }
        est.guardar_snapshot();
        est.lineas.remove(i);
        est.linea_activa = i - 1;
        guardar_archivo_fisico(&est.archivo_activo, &est.lineas);
        refrescar_eliminar();
        if let Some(ui) = ui_handle.upgrade() {
            ui.set_lineas_documento(ModelRc::from(reconstruir_modelo(&est)));
        }
    });

    let ui_handle = ui.as_weak();
    let est_c = estado.clone();
    let refrescar_undo = refrescar_resumen.clone();
    ui.on_undo(move || {
        let mut est = est_c.borrow_mut();
        if est.undo() {
            guardar_archivo_fisico(&est.archivo_activo, &est.lineas);
            refrescar_undo();
            if let Some(ui) = ui_handle.upgrade() {
                ui.set_lineas_documento(ModelRc::from(reconstruir_modelo(&est)));
            }
        }
    });

    let ui_handle = ui.as_weak();
    let est_c = estado.clone();
    let refrescar_redo = refrescar_resumen.clone();
    ui.on_redo(move || {
        let mut est = est_c.borrow_mut();
        if est.redo() {
            guardar_archivo_fisico(&est.archivo_activo, &est.lineas);
            refrescar_redo();
            if let Some(ui) = ui_handle.upgrade() {
                ui.set_lineas_documento(ModelRc::from(reconstruir_modelo(&est)));
            }
        }
    });

    let est_c = estado.clone();
    ui.on_exportar_nota(move || {
        let est = est_c.borrow();
        let contenido = est.lineas.join("\n");
        drop(est);
        if let Ok(mut cb) = Clipboard::new() {
            cb.set_text(contenido).ok();
        }
    });

    let ui_handle = ui.as_weak();
    let est_c = estado.clone();
    let modelo_nombres_del = modelo_nombres.clone();
    let nombre_actual_del = nombre_actual.clone();
    let refrescar_nota_del = refrescar_resumen.clone();
    ui.on_eliminar_nota(move |nombre| {
        if eliminar_archivo_fisico(nombre.as_str()) {
            if let Some(pos) = (0..modelo_nombres_del.row_count())
                .find(|&j| modelo_nombres_del.row_data(j).map(|e| e.nombre) == Some(nombre.clone()))
            {
                modelo_nombres_del.remove(pos);
            }
            let mut est = est_c.borrow_mut();
            if est.archivo_activo == nombre.as_str() {
                est.archivo_activo.clear();
                est.lineas.clear();
                est.historial.clear();
                est.historial_pos = 0;
            }
            let lista_actualizada: Vec<String> = (0..modelo_nombres_del.row_count())
                .filter_map(|j| modelo_nombres_del.row_data(j).map(|e| e.nombre.to_string()))
                .collect();
            let puede = puede_crear_semana_actual(&lista_actualizada, &nombre_actual_del);
            if let Some(ui) = ui_handle.upgrade() {
                if ui.get_nota_activa() == nombre {
                    ui.set_nota_activa(SharedString::from(""));
                    ui.set_lineas_documento(ModelRc::from(Rc::new(
                        VecModel::<LineaNota>::default(),
                    )));
                }
                ui.set_puede_crear_nota(puede);
            }
            refrescar_nota_del();
        }
    });

    ui.run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    #[test]
    fn parsear_nombre_valido() {
        assert_eq!(parsear_año_semana("2025-W20.md"), Some((2025, 20)));
        assert_eq!(parsear_año_semana("2024-W01.md"), Some((2024, 1)));
        assert_eq!(parsear_año_semana("2026-W52.md"), Some((2026, 52)));
    }

    #[test]
    fn parsear_nombre_invalido() {
        assert_eq!(parsear_año_semana("nota.md"), None);
        assert_eq!(parsear_año_semana("2025.md"), None);
        assert_eq!(parsear_año_semana("2025-20.md"), None);
        assert_eq!(parsear_año_semana(""), None);
    }

    #[test]
    fn primera_nota_siempre_permitida() {
        assert!(puede_crear_semana_actual(&[], "2025-W20.md"));
    }

    #[test]
    fn no_puede_duplicar_semana_actual() {
        let notas = vec!["2025-W20.md".to_string()];
        assert!(!puede_crear_semana_actual(&notas, "2025-W20.md"));
    }

    #[test]
    fn puede_crear_semana_nueva() {
        let notas = vec!["2025-W19.md".to_string()];
        assert!(puede_crear_semana_actual(&notas, "2025-W20.md"));
    }

    #[test]
    fn semana_actual_bloquea_aunque_haya_otras() {
        let notas = vec!["2025-W20.md".to_string(), "2025-W19.md".to_string()];
        assert!(!puede_crear_semana_actual(&notas, "2025-W20.md"));
    }

    #[test]
    fn vinieta_limpia_sin_corromper_cuerpo() {
        let estado = EstadoEditor {
            archivo_activo: String::new(),
            lineas: vec![
                "* Texto con - guion".to_string(),
                "- Texto con * asterisco".to_string(),
            ],
            linea_activa: 999,
            historial: Vec::new(),
            historial_pos: 0,
        };
        let modelo = reconstruir_modelo(&estado);
        assert_eq!(
            modelo.row_data(0).unwrap().texto_limpio.as_str(),
            "Texto con - guion"
        );
        assert_eq!(
            modelo.row_data(1).unwrap().texto_limpio.as_str(),
            "Texto con * asterisco"
        );
    }

    #[test]
    fn tipos_markdown_correctos() {
        assert_eq!(determinar_tipo("# Título"), "titulo1");
        assert_eq!(determinar_tipo("## Subtítulo"), "titulo2");
        assert_eq!(determinar_tipo("- [ ] Tarea"), "tarea_pendiente");
        assert_eq!(determinar_tipo("- [x] Hecho"), "tarea_completada");
        assert_eq!(determinar_tipo("- [X] Hecho"), "tarea_completada");
        assert_eq!(determinar_tipo("- Viñeta"), "vinieta");
        assert_eq!(determinar_tipo("* Viñeta"), "vinieta");
        assert_eq!(determinar_tipo("Párrafo normal"), "parrafo");
        assert_eq!(determinar_tipo(""), "parrafo");
    }
}

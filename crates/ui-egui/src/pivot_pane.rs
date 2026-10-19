//! The PivotTable Fields task pane: shown on the right when the active cell is in a pivot
//! report. Ticking a field adds it (text → Rows, numbers → Values). Like Excel, fields are
//! dragged from the list into Filters, Columns, Rows or Values, between areas (and within one to
//! reorder), and back onto the list to remove them; each area field also has a menu. Report
//! filters open the item checklist (`pivot_filter`). Everything runs `pivot.*` commands.

use egui::{Color32, Ui, vec2};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::i18n;
use crate::theme::{self, Tokens};

/// Σ Values (two or more value fields) sits in Rows or Columns; it moves only between them.
const VALUES_FIELD: &str = "Σ Values";

/// What is being dragged: a field from the list (`from: None`) or from an area.
#[derive(Clone, Debug, PartialEq)]
struct Dragged {
    field: String,
    from: Option<&'static str>,
}

/// Whether the active cell is inside a pivot report (so the pane should show).
pub fn active_pivot(app: &SheetApp) -> Option<String> {
    let d = app.session.active()?;
    let sh = d.wb.active()?;
    let a = d.selection.active;
    sh.pivots.iter().find(|p| p.last_range.is_some_and(|r| r.contains(a)) || p.anchor == a).map(|p| p.name.clone())
}

/// The command a drop runs: `to` is an area or `"list"` (remove); `areas` are the areas the
/// field is in now (non-value areas first). `None` when nothing changes.
fn drop_command(pivot: &str, d: &Dragged, to: &str, position: Option<usize>, areas: &[String]) -> Option<(&'static str, Json)> {
    let pos = |mut v: Json| {
        if let Some(p) = position {
            v["position"] = json!(p);
        }
        v
    };
    match d.from {
        // From the field list.
        None => {
            if to == "list" {
                return None;
            }
            // A field already on an axis or the filters moves; Values may hold it a second time.
            let current = areas.iter().find(|a| a.as_str() != "values");
            match current {
                Some(a) if to != "values" => {
                    (a != to || position.is_some()).then(|| ("pivot.moveField", pos(json!({"pivot": pivot, "field": d.field, "from": a, "to": to}))))
                }
                _ if to == "values" && areas.iter().any(|a| a == "values") => None,
                _ => Some(("pivot.addField", pos(json!({"pivot": pivot, "field": d.field, "area": to})))),
            }
        }
        Some(from) => {
            if to == "list" {
                return (d.field != VALUES_FIELD).then(|| ("pivot.removeField", json!({"pivot": pivot, "field": d.field, "area": from})));
            }
            if d.field == VALUES_FIELD && !matches!(to, "rows" | "columns") {
                return None;
            }
            if from == to && position.is_none() {
                return None;
            }
            Some(("pivot.moveField", pos(json!({"pivot": pivot, "field": d.field, "from": from, "to": to}))))
        }
    }
}

pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let Some(name) = active_pivot(app) else { return };
    let t = Tokens::get(ui.ctx());
    let lang = i18n::current(ui.ctx());
    let info = match app.session.run("pivot.fields", json!({"pivot": name})) {
        Ok(v) => v,
        Err(_) => return,
    };
    let mut command: Option<(&'static str, Json)> = None;
    let mut filter_items: Option<String> = None;
    egui::Panel::right("pivot_pane").default_size(280.0).resizable(true).frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(10)).show(ui, |ui| {
        ui.label(egui::RichText::new(lang.tr("PivotTable Fields")).font(theme::ui_bold(15.0)));
        ui.label(egui::RichText::new(lang.tr("Choose fields to add to report:")).small().color(t.text_dim));
        ui.add_space(4.0);
        let fields = info["fields"].as_array().cloned().unwrap_or_default();
        let areas_of = |field: &str| -> Vec<String> {
            fields
                .iter()
                .find(|f| f["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(field)))
                .and_then(|f| f["areas"].as_array())
                .map(|a| {
                    let mut v: Vec<String> = a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
                    v.sort_by_key(|a| a == "values");
                    v
                })
                .unwrap_or_default()
        };

        // Field list: tick to add, drag into an area; dropping an area field here removes it.
        let list_frame = egui::Frame::NONE.inner_margin(2.0);
        let (_, dropped) = ui.dnd_drop_zone::<Dragged, ()>(list_frame, |ui| {
            egui::ScrollArea::vertical().max_height(220.0).id_salt("pv_fields").show(ui, |ui| {
                for f in &fields {
                    let fname = f["name"].as_str().unwrap_or("").to_string();
                    let areas: Vec<String> =
                        f["areas"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    let mut on = !areas.is_empty();
                    let numeric = f["numeric"].as_bool().unwrap_or(false);
                    let samples: Vec<String> =
                        f["samples"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    let drag = Dragged { field: fname.clone(), from: None };
                    ui.dnd_drag_source(egui::Id::new(("pv_field", &fname)), drag, |ui| {
                        let r = ui.checkbox(&mut on, egui::RichText::new(&fname).font(theme::ui_font(13.0)));
                        if r.changed() {
                            command = Some(if on {
                                ("pivot.addField", json!({"pivot": name, "field": fname, "area": if numeric { "values" } else { "rows" }}))
                            } else {
                                ("pivot.removeField", json!({"pivot": name, "field": fname}))
                            });
                        }
                        r.on_hover_text(samples.join(", "));
                    });
                }
            });
        });
        if let Some(d) = dropped {
            command = drop_command(&name, &d, "list", None, &areas_of(&d.field)).or(command.take());
        }
        ui.separator();
        ui.label(egui::RichText::new(lang.tr("Drag fields between areas below:")).small().color(t.text_dim));

        let area = |key: &str| -> Vec<String> {
            let list = info["pivot"][key].as_array().or(info[key].as_array());
            list.map(|a| {
                a.iter()
                    // Hidden items of an axis field are kept as filters too; only report filters show here.
                    .filter(|x| key != "filters" || x["report"].as_bool() != Some(false))
                    .filter_map(|x| x["name"].as_str().or(x["field"].as_str()).or(x["sourceCol"].as_str()).or(x.as_str()).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
        };
        let values_key = if info["valuesOnRows"].as_bool().unwrap_or(false) { "rows" } else { "columns" };
        let values_at = info["valuesPosition"].as_u64().map(|p| p as usize);
        let area_with_values = |key: &str| -> Vec<String> {
            let mut v = area(key);
            if key == values_key && area("values").len() > 1 {
                let at = values_at.unwrap_or(v.len()).min(v.len());
                v.insert(at, VALUES_FIELD.to_string());
            }
            v
        };
        let dragging = egui::DragAndDrop::has_payload_of_type::<Dragged>(ui.ctx());
        ui.columns(2, |cols| {
            for (i, (key, label)) in [("filters", "Filters"), ("columns", "Columns"), ("rows", "Rows"), ("values", "Values")].iter().enumerate() {
                let ui = &mut cols[i % 2];
                ui.label(egui::RichText::new(lang.tr(label)).strong());
                let frame = egui::Frame::NONE
                    .fill(t.window)
                    .stroke(egui::Stroke::new(1.0, if dragging { Color32::from_rgb(0x21, 0x73, 0x46) } else { Color32::TRANSPARENT }))
                    .corner_radius(4.0)
                    .inner_margin(4.0);
                let mut chip_rects: Vec<egui::Rect> = vec![];
                let (zone, dropped) = ui.dnd_drop_zone::<Dragged, ()>(frame, |ui| {
                    ui.set_min_size(vec2(ui.available_width(), 60.0));
                    for f in area_with_values(key) {
                        let is_values = f == VALUES_FIELD;
                        let drag = Dragged { field: f.clone(), from: Some(*key) };
                        let chip = ui.dnd_drag_source(egui::Id::new(("pv_area", *key, &f)), drag, |ui| {
                            ui.add(
                                egui::Button::new(egui::RichText::new(format!("{f} ▾")).font(theme::ui_font(12.0)))
                                    .fill(Color32::WHITE)
                                    .min_size(vec2(ui.available_width(), 20.0)),
                            )
                        });
                        chip_rects.push(chip.response.rect);
                        let b = chip.inner;
                        egui::Popup::menu(&b).show(|ui| {
                            if *key == "filters" && ui.button(lang.tr("Filter Items…")).clicked() {
                                filter_items = Some(f.clone());
                            }
                            for (to, l) in [
                                ("rows", "Move to Row Labels"),
                                ("columns", "Move to Column Labels"),
                                ("filters", "Move to Report Filter"),
                                ("values", "Move to Values"),
                            ] {
                                if to != *key && (!is_values || matches!(to, "rows" | "columns")) && ui.button(lang.tr(l)).clicked() {
                                    command = Some(("pivot.moveField", json!({"pivot": name, "field": f, "from": key, "to": to})));
                                }
                            }
                            if is_values {
                                return;
                            }
                            if *key == "values" {
                                ui.separator();
                                for (func, l) in [
                                    ("sum", "Summarize by Sum"),
                                    ("count", "Summarize by Count"),
                                    ("average", "Summarize by Average"),
                                    ("max", "Summarize by Max"),
                                    ("min", "Summarize by Min"),
                                ] {
                                    if ui.button(lang.tr(l)).clicked() {
                                        command = Some(("pivot.valueSettings", json!({"pivot": name, "field": f, "func": func})));
                                    }
                                }
                                for (sa, l) in [
                                    ("normal", "Show as No Calculation"),
                                    ("percentOfGrandTotal", "Show as % of Grand Total"),
                                    ("runningTotal", "Show as Running Total"),
                                    ("rank", "Show as Rank"),
                                ] {
                                    if ui.button(lang.tr(l)).clicked() {
                                        command = Some(("pivot.valueSettings", json!({"pivot": name, "field": f, "showAs": sa})));
                                    }
                                }
                            } else {
                                ui.separator();
                                for (o, l) in [("asc", "Sort A to Z"), ("desc", "Sort Z to A")] {
                                    if ui.button(lang.tr(l)).clicked() {
                                        command = Some(("pivot.sort", json!({"pivot": name, "field": f, "order": o})));
                                    }
                                }
                                for (g, l) in [
                                    ("years", "Group by Years"),
                                    ("quarters", "Group by Quarters"),
                                    ("months", "Group by Months"),
                                    ("none", "Ungroup"),
                                ] {
                                    if ui.button(lang.tr(l)).clicked() {
                                        command = Some(("pivot.group", json!({"pivot": name, "field": f, "by": g})));
                                    }
                                }
                            }
                            ui.separator();
                            if ui.button(lang.tr("Remove Field")).clicked() {
                                command = Some(("pivot.removeField", json!({"pivot": name, "field": f, "area": key})));
                            }
                        });
                    }
                });
                // Where the field would land: before the first chip whose middle is below the pointer.
                let pointer = ui.ctx().pointer_interact_pos();
                let position = pointer.map(|p| chip_rects.iter().position(|r| p.y < r.center().y).unwrap_or(chip_rects.len()));
                if dragging
                    && zone.response.contains_pointer()
                    && let Some(at) = position
                {
                    let y = chip_rects.get(at).map_or_else(|| chip_rects.last().map_or(zone.response.rect.top() + 6.0, |r| r.bottom() + 2.0), |r| r.top() - 2.0);
                    let x = zone.response.rect.x_range().shrink(4.0);
                    ui.painter().hline(x, y, egui::Stroke::new(2.0, Color32::from_rgb(0x21, 0x73, 0x46)));
                }
                if let Some(d) = dropped {
                    command = drop_command(&name, &d, key, position, &areas_of(&d.field));
                }
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(lang.tr("Refresh")).clicked() {
                command = Some(("pivot.refresh", json!({"pivot": name})));
            }
            if ui.button(lang.tr("Calculated Field…")).clicked() {
                command = Some(("pivot.calculatedField", json!({"pivot": name})));
            }
            egui::ComboBox::from_id_salt("pv_layout").selected_text(lang.tr("Report Layout")).show_ui(ui, |ui| {
                for (l, v) in [("Compact", "compact"), ("Outline", "outline"), ("Tabular", "tabular")] {
                    if ui.button(lang.tr(l)).clicked() {
                        command = Some(("pivot.layout", json!({"pivot": name, "layout": v})));
                    }
                }
            });
        });
    });
    if let Some((id, params)) = command {
        app.run_or_alert(id, params);
    }
    if let Some(field) = filter_items
        && let Some(sh) = app.session.active().and_then(|d| d.wb.active())
        && let Some(cell) = crate::pivot_filter::filter_cells(sh).into_iter().find(|c| c.pivot == name && c.field.eq_ignore_ascii_case(&field))
    {
        crate::pivot_filter::open(app, cell);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drag(field: &str, from: Option<&'static str>) -> Dragged {
        Dragged { field: field.into(), from }
    }

    #[test]
    fn drops_add_move_reorder_and_remove_like_excel() {
        let none: Vec<String> = vec![];
        // List → area adds; list → list does nothing.
        let (id, p) = drop_command("PT", &drag("Unidade", None), "filters", None, &none).unwrap();
        assert_eq!((id, p["area"].as_str()), ("pivot.addField", Some("filters")));
        assert!(drop_command("PT", &drag("Unidade", None), "list", None, &none).is_none());
        // List → another area for a field already on rows moves it.
        let rows = vec!["rows".to_string()];
        let (id, p) = drop_command("PT", &drag("Unidade", None), "filters", None, &rows).unwrap();
        assert_eq!((id, p["from"].as_str(), p["to"].as_str()), ("pivot.moveField", Some("rows"), Some("filters")));
        // ... but into Values it is added a second time (count/sum of it).
        let (id, _) = drop_command("PT", &drag("Unidade", None), "values", None, &rows).unwrap();
        assert_eq!(id, "pivot.addField");
        // Area → area moves, with the drop position.
        let (id, p) = drop_command("PT", &drag("Funcao", Some("rows")), "columns", Some(0), &rows).unwrap();
        assert_eq!((id, p["to"].as_str(), p["position"].as_u64()), ("pivot.moveField", Some("columns"), Some(0)));
        // Within one area: reorder only with a position.
        assert!(drop_command("PT", &drag("Funcao", Some("rows")), "rows", None, &rows).is_none());
        assert_eq!(drop_command("PT", &drag("Funcao", Some("rows")), "rows", Some(1), &rows).unwrap().1["position"], 1);
        // Area → list removes; Σ Values only moves between Rows and Columns.
        let (id, p) = drop_command("PT", &drag("Funcao", Some("rows")), "list", None, &rows).unwrap();
        assert_eq!((id, p["area"].as_str()), ("pivot.removeField", Some("rows")));
        assert!(drop_command("PT", &drag(VALUES_FIELD, Some("columns")), "filters", None, &none).is_none());
        assert!(drop_command("PT", &drag(VALUES_FIELD, Some("columns")), "list", None, &none).is_none());
        assert!(drop_command("PT", &drag(VALUES_FIELD, Some("columns")), "rows", None, &none).is_some());
    }

    #[test]
    fn dropped_commands_run_on_a_real_pivot() {
        let mut s = gridcraft_engine::Session::new();
        s.new_workbook();
        let mut app = SheetApp::new(s, Default::default());
        let rows = json!([["Unidade", "Funcao", "Qtd"], ["A", "X", 1], ["B", "Y", 2], ["A", "Y", 3]]);
        app.run("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        app.run("selection.set", json!({"range": "A1:C4"})).unwrap();
        app.run("insert.pivotTable", json!({"destination": "Sheet1!F3", "rows": ["Funcao", "Unidade"]})).unwrap();
        let name = app.session.run("pivot.list", json!({})).unwrap()[0]["name"].as_str().unwrap().to_string();
        // Drag Unidade from Rows to Filters: it becomes a report filter with an "(All)" cell.
        let (id, p) = drop_command(&name, &drag("Unidade", Some("rows")), "filters", None, &["rows".to_string()]).unwrap();
        app.run(id, p).unwrap();
        let sh = app.session.active().unwrap().wb.active().unwrap().clone();
        let cells = crate::pivot_filter::filter_cells(&sh);
        assert_eq!(cells.iter().map(|c| c.field.as_str()).collect::<Vec<_>>(), ["Unidade"]);
        // Drag Qtd from the list to Values.
        let (id, p) = drop_command(&name, &drag("Qtd", None), "values", None, &[]).unwrap();
        app.run(id, p).unwrap();
        let f = app.session.run("pivot.fields", json!({"pivot": name})).unwrap();
        assert_eq!(f["values"].as_array().map(Vec::len), Some(1));
        assert_eq!(f["rows"].as_array().map(Vec::len), Some(1));
    }
}

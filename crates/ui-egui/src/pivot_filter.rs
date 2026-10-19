//! Report filter drop-downs of PivotTables, like Excel's: a ▾ button in each filter's value cell
//! (the "(All)" cell above the report) opens a checklist of the field's items with a search box
//! and (Select All); OK runs `pivot.filter`. The Filters area of the PivotTable Fields pane opens
//! the same list.

use egui::{Align2, Color32, Painter, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::Sheet;
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::grid::Geo;

/// One report filter of a pivot on the sheet and the cell showing its selection.
#[derive(Clone, Debug, PartialEq)]
pub struct FilterCell {
    pub pivot: String,
    pub field: String,
    pub cell: CellRef,
}

/// The open checklist.
#[derive(Clone, Debug)]
pub struct Open {
    pub at: FilterCell,
    items: Vec<String>,
    checked: Vec<bool>,
    search: String,
}

/// Report filters (fields in the Filters area that are not also on an axis) with their value
/// cells: filter `i` sits on the report's row `i`, its selection in the second column.
pub fn filter_cells(sh: &Sheet) -> Vec<FilterCell> {
    let mut out = vec![];
    for pt in &sh.pivots {
        let Some(range) = pt.last_range else { continue };
        let on_axis = |f: &str| pt.rows.iter().chain(pt.columns.iter()).any(|x| x.source_col.eq_ignore_ascii_case(f));
        for (i, f) in pt.filters.iter().filter(|f| !on_axis(&f.source_col)).enumerate() {
            out.push(FilterCell { pivot: pt.name.clone(), field: f.source_col.clone(), cell: CellRef::new(range.start.row + i as u32, range.start.col + 1) });
        }
    }
    out
}

/// The ▾ button inside the right edge of a filter's value cell.
pub fn arrow_rect(sh: &Sheet, geo: &Geo, c: CellRef) -> Option<Rect> {
    if geo.hidden_under_frozen(sh, c) {
        return None;
    }
    let r = geo.cell_rect(sh, c);
    let s = r.height().clamp(12.0, 18.0);
    Some(Rect::from_min_size(pos2(r.right() - s - 1.0, r.bottom() - s - 1.0), vec2(s, s)))
}

pub fn paint_arrows(p: &Painter, sh: &Sheet, geo: &Geo) {
    for f in filter_cells(sh) {
        if let Some(r) = arrow_rect(sh, geo, f.cell) {
            p.rect_filled(r, 2.0, Color32::from_gray(245));
            p.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_gray(160)), StrokeKind::Inside);
            crate::icons::paint(p, r.shrink(3.0), crate::icons::Icon::Chevron, Color32::from_gray(60));
        }
    }
}

/// The filter whose ▾ button is under `p`.
pub fn hit(sh: &Sheet, geo: &Geo, p: Pos2) -> Option<FilterCell> {
    filter_cells(sh).into_iter().find(|f| arrow_rect(sh, geo, f.cell).is_some_and(|r| r.contains(p)))
}

/// Opens the checklist for `at`, loading the field's items and current selection.
pub fn open(app: &mut SheetApp, at: FilterCell) {
    let Ok(v) = app.session.run("pivot.items", json!({"pivot": at.pivot, "field": at.field})) else { return };
    let items: Vec<String> = v["items"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let selected: Option<Vec<String>> = v["selected"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect());
    let checked = items.iter().map(|it| selected.as_ref().is_none_or(|s| s.iter().any(|x| x.eq_ignore_ascii_case(it)))).collect();
    app.grid.pivot_filter = Some(Open { at, items, checked, search: String::new() });
}

/// The selection to send: `None` when every item is kept ("(All)").
fn selection(o: &Open) -> Option<Vec<String>> {
    if o.checked.iter().all(|c| *c) {
        return None;
    }
    Some(o.items.iter().zip(&o.checked).filter(|(_, c)| **c).map(|(i, _)| i.clone()).collect())
}

pub fn popup(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, sh: &Sheet, was_open: bool) {
    let Some(mut o) = app.grid.pivot_filter.take() else { return };
    let pt = app.ui.language == crate::i18n::Language::PtBr;
    let t = |p: &'static str, e: &'static str| if pt { p } else { e };
    let r = geo.cell_rect(sh, o.at.cell);
    let (at, pivot) = (geo.mirror_pos(r.left_bottom()), if geo.rtl { Align2::RIGHT_TOP } else { Align2::LEFT_TOP });
    let mut close = false;
    let mut apply = false;
    let response = egui::Area::new(egui::Id::new("pivot_filter")).fixed_pos(at).pivot(pivot).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).inner_margin(6.0).show(ui, |ui| {
            ui.set_width(220.0);
            ui.add(egui::TextEdit::singleline(&mut o.search).hint_text(t("Pesquisar", "Search")).desired_width(f32::INFINITY));
            ui.add_space(4.0);
            let needle = o.search.trim().to_lowercase();
            let shown: Vec<usize> = (0..o.items.len()).filter(|&i| needle.is_empty() || o.items[i].to_lowercase().contains(&needle)).collect();
            egui::Frame::NONE.stroke(Stroke::new(1.0, Color32::from_gray(200))).inner_margin(4.0).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    let mut all = !shown.is_empty() && shown.iter().all(|&i| o.checked[i]);
                    if ui.checkbox(&mut all, t("(Selecionar Tudo)", "(Select All)")).changed() {
                        for &i in &shown {
                            o.checked[i] = all;
                        }
                    }
                    for &i in &shown {
                        let label = o.items[i].clone();
                        ui.checkbox(&mut o.checked[i], label);
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let any = o.checked.iter().any(|c| *c);
                if ui.add_enabled(any, egui::Button::new("OK")).clicked() {
                    apply = true;
                    close = true;
                }
                if ui.button(t("Cancelar", "Cancel")).clicked() {
                    close = true;
                }
            });
        });
    });
    if apply {
        let selected = selection(&o).map_or(Json::Null, |s| json!(s));
        app.run_or_alert("pivot.filter", json!({"pivot": o.at.pivot, "field": o.at.field, "selected": selected}));
    }
    let dismissed = response.response.ctx.input(|i| !i.focused || i.key_pressed(egui::Key::Escape)) || (was_open && response.response.clicked_elsewhere());
    if !close && !dismissed {
        app.grid.pivot_filter = Some(o);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_filters_have_value_cells_and_open_with_their_items() {
        let mut s = gridcraft_engine::Session::new();
        s.new_workbook();
        let mut app = SheetApp::new(s, Default::default());
        let rows = json!([["Region", "Product", "Sales"], ["East", "Apples", 10], ["West", "Pears", 20], ["East", "Pears", 5]]);
        app.run("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        app.run("selection.set", json!({"range": "A1:C4"})).unwrap();
        app.run("insert.pivotTable", json!({"destination": "Sheet1!F3", "rows": ["Product"], "values": ["Sales"], "filters": ["Region"]})).unwrap();
        let sh = app.session.active().unwrap().wb.active().unwrap().clone();
        let cells = filter_cells(&sh);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].field, "Region");
        assert_eq!(gridcraft_engine::display::cell_text(&app.session.active().unwrap().wb, &sh, cells[0].cell), "(All)");

        open(&mut app, cells[0].clone());
        let o = app.grid.pivot_filter.clone().unwrap();
        assert_eq!(o.items, ["East", "West"]);
        assert!(o.checked.iter().all(|c| *c));
        assert_eq!(selection(&o), None);
        let mut only_east = o.clone();
        only_east.checked = vec![true, false];
        assert_eq!(selection(&only_east), Some(vec!["East".to_string()]));
        app.run("pivot.filter", json!({"pivot": o.at.pivot, "field": "Region", "selected": ["East"]})).unwrap();
        let wb = app.session.active().unwrap().wb.clone();
        assert_eq!(gridcraft_engine::display::cell_text(&wb, wb.active().unwrap(), cells[0].cell), "East");
        // Reopening shows the filter's own selection.
        open(&mut app, cells[0].clone());
        assert_eq!(app.grid.pivot_filter.as_ref().unwrap().checked, [true, false]);
    }
}

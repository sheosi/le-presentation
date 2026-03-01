pub fn main_dashboard() -> String {
    format!(
        r#"<html><head><meta charset=UTF-8></head><body>
        <h1>Sistema de presentaciones</h1><form method=POST action=/dashboard>
        <a href="/presentation">Ir a la presentación</a>
        </form></body></html>"#
    )
}

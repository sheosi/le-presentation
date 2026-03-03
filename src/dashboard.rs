pub fn main_dashboard() -> String {
    format!(
        r#"<html><head><meta charset=UTF-8></head><body>
        <h1>Sistema de presentaciones</h1>
        <a href="/presentation">Ir a la presentación</a>

        <form method="POST" action="/settings/limiter">
            <input type="submit">Limiter</input>
        </form>

        </body></html>"#
    )
}

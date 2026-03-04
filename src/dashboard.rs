pub fn main_dashboard(limiter_on: bool) -> String {
    format!(
        r#"<html><head><meta charset=UTF-8></head><body>
        <h1>Sistema de presentaciones</h1>
        <a href="/presentation">Ir a la presentación</a>

        <form method="POST" action="/settings/limiter" id="limiterForm">
            <label>
                <input type="checkbox" {} name="enable" onchange="this.form.submit()">
                Limiter
            </label>
        </form>

        </body></html>"#,
        if limiter_on { "checked" } else { "" }
    )
}

pub mod sys_integration;

pub fn main_dashboard(limiter_on: bool) -> String {
    let volume = sys_integration::get_current_volume();
    format!(
        r#"<!DOCTYPE html>
        <html lang="es">
        <head>
          <meta charset="UTF-8">
          <meta name="viewport" content="width=device-width, initial-scale=1.0">
          <title>Sistema de Presentaciones</title>
          <link rel="preconnect" href="https://fonts.googleapis.com">
          <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
          <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600&display=swap" rel="stylesheet">
          <style>
            *, *::before, *::after {{
              margin: 0;
              padding: 0;
              box-sizing: border-box;
            }}

            :root {{
              --bg: #1a1a1a;
              --fg: #e8e8e8;
              --card: #232323;
              --card-fg: #e8e8e8;
              --secondary: #2e2e2e;
              --secondary-fg: #d4d4d4;
              --muted-fg: #737373;
              --muted-fg-dim: rgba(115, 115, 115, 0.6);
              --accent: #34d399;
              --accent-dim: rgba(52, 211, 153, 0.1);
              --accent-border: rgba(52, 211, 153, 0.3);
              --border: #333;
              --border-dim: rgba(51, 51, 51, 0.6);
              --radius: 0.5rem;
            }}

            body {{
              font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
              background-color: var(--bg);
              color: var(--fg);
              min-height: 100dvh;
              -webkit-font-smoothing: antialiased;
              -moz-osx-font-smoothing: grayscale;
            }}

            /* Layout */
            .container {{
              max-width: 672px;
              margin: 0 auto;
              padding: 1.5rem 1rem;
            }}

            /* Header */
            .header {{
              display: flex;
              flex-direction: column;
              gap: 1rem;
            }}

            .header-info {{
              display: flex;
              align-items: center;
              gap: 0.75rem;
            }}

            .header-icon {{
              flex-shrink: 0;
              width: 2.25rem;
              height: 2.25rem;
              display: flex;
              align-items: center;
              justify-content: center;
              border-radius: var(--radius);
              background: var(--accent-dim);
              color: var(--accent);
            }}

            .header-text {{
              min-width: 0;
            }}

            .header-title-row {{
              display: flex;
              align-items: center;
              gap: 0.5rem;
            }}

            .header-title {{
              font-size: 1rem;
              font-weight: 600;
              letter-spacing: -0.01em;
              color: var(--fg);
              overflow: hidden;
              text-overflow: ellipsis;
              white-space: nowrap;
            }}

            .badge {{
              flex-shrink: 0;
              font-size: 0.75rem;
              font-weight: 500;
              padding: 0.125rem 0.5rem;
              border-radius: 9999px;
              border: 1px solid var(--accent-border);
              color: var(--accent);
              line-height: 1.4;
            }}

            .header-subtitle {{
              font-size: 0.75rem;
              color: var(--muted-fg);
              margin-top: 0.125rem;
            }}

            .btn-presentation {{
              display: inline-flex;
              width: 100%;
              align-items: center;
              justify-content: center;
              gap: 0.5rem;
              padding: 0.625rem 1rem;
              font-size: 0.875rem;
              font-weight: 500;
              font-family: inherit;
              color: var(--fg);
              background: var(--secondary);
              border: 1px solid var(--border);
              border-radius: var(--radius);
              cursor: pointer;
              text-decoration: none;
              transition: background 0.15s, border-color 0.15s;
            }}

            .btn-presentation:hover {{
              background: rgba(46, 46, 46, 0.8);
              border-color: var(--accent-border);
            }}

            .btn-presentation:hover .btn-icon {{
              color: var(--accent);
            }}

            .btn-presentation:hover .btn-arrow {{
              transform: translateX(2px);
            }}

            .btn-icon, .btn-arrow {{
              color: var(--muted-fg);
              transition: color 0.15s, transform 0.15s;
              flex-shrink: 0;
            }}

            /* Separator */
            .separator {{
              height: 1px;
              background: var(--border);
              border: none;
              margin: 1.25rem 0;
            }}

            /* Section */
            .section-label {{
              font-size: 0.75rem;
              font-weight: 500;
              text-transform: uppercase;
              letter-spacing: 0.05em;
              color: var(--muted-fg);
            }}

            .section-desc {{
              font-size: 0.75rem;
              color: var(--muted-fg-dim);
              margin-top: 0.375rem;
            }}

            .settings-list {{
              margin-top: 1rem;
              display: flex;
              flex-direction: column;
              gap: 1rem;
            }}

            /* Limiter Card (clickable) */
            .limiter-card {{
              display: flex;
              width: 100%;
              align-items: center;
              gap: 0.75rem;
              padding: 1rem;
              text-align: left;
              background: var(--card);
              border: 1px solid var(--border-dim);
              border-radius: 0.75rem;
              cursor: pointer;
              font-family: inherit;
              color: inherit;
              transition: background 0.15s, border-color 0.15s;
            }}

            .limiter-card:hover {{
              border-color: var(--accent-border);
              background: rgba(35, 35, 35, 0.8);
            }}

            .limiter-card:active {{
              background: var(--secondary);
            }}

            .limiter-card:disabled {{
              opacity: 0.6;
              pointer-events: none;
            }}

            .limiter-card:hover .limiter-icon-box {{
              background: var(--accent-dim);
            }}

            .limiter-card:hover .limiter-icon-box svg {{
              color: var(--accent);
            }}

            .limiter-icon-box {{
              flex-shrink: 0;
              width: 2.25rem;
              height: 2.25rem;
              display: flex;
              align-items: center;
              justify-content: center;
              border-radius: var(--radius);
              background: var(--secondary);
              transition: background 0.15s;
            }}

            .limiter-icon-box svg {{
              color: var(--muted-fg);
              transition: color 0.15s;
            }}

            .limiter-text {{
              flex: 1;
              min-width: 0;
            }}

            .limiter-title {{
              font-size: 0.875rem;
              font-weight: 600;
              color: var(--card-fg);
              display: block;
            }}

            .limiter-desc {{
              font-size: 0.75rem;
              color: var(--muted-fg);
              display: block;
              margin-top: 0.125rem;
            }}

            /* Volume Card */
            .volume-card {{
              display: flex;
              width: 100%;
              align-items: center;
              gap: 0.75rem;
              padding: 1rem;
              background: var(--card);
              border: 1px solid var(--border-dim);
              border-radius: 0.75rem;
              transition: border-color 0.15s;
            }}

            .volume-card:hover {{
              border-color: var(--accent-border);
            }}

            .volume-icon-box {{
              flex-shrink: 0;
              width: 2.25rem;
              height: 2.25rem;
              display: flex;
              align-items: center;
              justify-content: center;
              border-radius: var(--radius);
              background: var(--secondary);
            }}

            .volume-icon-box svg {{
              color: var(--muted-fg);
            }}

            .volume-text {{
              flex: 1;
              min-width: 0;
            }}

            .volume-title {{
              font-size: 0.875rem;
              font-weight: 600;
              color: var(--card-fg);
              display: block;
            }}

            .volume-level {{
              font-size: 0.75rem;
              color: var(--muted-fg);
              display: block;
              margin-top: 0.125rem;
            }}

            .volume-controls {{
              display: flex;
              align-items: center;
              gap: 0.5rem;
            }}

            .volume-btn {{
              width: 2rem;
              height: 2rem;
              display: flex;
              align-items: center;
              justify-content: center;
              border-radius: var(--radius);
              background: var(--secondary);
              border: 1px solid var(--border);
              color: var(--fg);
              font-size: 1rem;
              font-weight: 600;
              cursor: pointer;
              transition: background 0.15s, border-color 0.15s;
            }}

            .volume-btn:hover {{
              background: rgba(46, 46, 46, 0.8);
              border-color: var(--accent-border);
            }}

            .volume-btn:active {{
              background: var(--accent);
            }}

            .volume-card.disabled {{
              opacity: 0.5;
              pointer-events: none;
            }}

            .volume-card.disabled .volume-btn {{
              background: var(--secondary);
              border-color: var(--border);
              cursor: not-allowed;
            }}

            @media (min-width: 640px) {{
              .volume-card {{
                gap: 1rem;
                padding: 1.25rem;
              }}

              .volume-icon-box {{
                width: 2.5rem;
                height: 2.5rem;
              }}

              .volume-title {{
                font-size: 1rem;
              }}

              .volume-level {{
                font-size: 0.875rem;
              }}

              .volume-btn {{
                width: 2.25rem;
                height: 2.25rem;
              }}
            }}

            /* Toggle switch */
            .toggle {{
              flex-shrink: 0;
              position: relative;
              width: 2.75rem;
              height: 1.5rem;
              border-radius: 9999px;
              background: var(--secondary);
              border: 1px solid var(--border);
              transition: background 0.2s, border-color 0.2s;
              pointer-events: none;
            }}

            .toggle.active {{
              background: var(--accent);
              border-color: var(--accent);
            }}

            .toggle-knob {{
              position: absolute;
              top: 2px;
              left: 2px;
              width: 1.125rem;
              height: 1.125rem;
              border-radius: 50%;
              background: white;
              transition: transform 0.2s;
              box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
            }}

            .toggle.active .toggle-knob {{
              transform: translateX(1.25rem);
            }}

            /* Responsive: sm (640px+) */
            @media (min-width: 640px) {{
              .container {{
                padding: 2.5rem 1.5rem;
              }}

              .header {{
                flex-direction: row;
                align-items: center;
                justify-content: space-between;
              }}

              .header-title {{
                font-size: 1.125rem;
              }}

              .header-subtitle {{
                font-size: 0.875rem;
              }}

              .btn-presentation {{
                width: auto;
                padding-top: 0.5rem;
                padding-bottom: 0.5rem;
              }}

              .separator {{
                margin: 2rem 0;
              }}

              .section-label {{
                font-size: 0.875rem;
              }}

              .section-desc {{
                font-size: 0.875rem;
              }}

              .settings-list {{
                margin-top: 1.5rem;
              }}

              .limiter-card {{
                gap: 1rem;
                padding: 1.25rem;
              }}

              .limiter-icon-box {{
                width: 2.5rem;
                height: 2.5rem;
              }}

              .limiter-title {{
                font-size: 1rem;
              }}

              .limiter-desc {{
                font-size: 0.875rem;
              }}
              }}

            /* Responsive: md (768px+) */
            @media (min-width: 768px) {{
              .container {{
                padding: 3rem 1.5rem;
              }}
              }}
          </style>
        </head>
        <body>
          <div class="container">

            <!-- Header -->
            <header class="header">
              <div class="header-info">
                <div class="header-icon">
                  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <rect x="2" y="3" width="20" height="14" rx="2"></rect>
                    <path d="M8 21h8"></path>
                    <path d="M12 17v4"></path>
                  </svg>
                </div>
                <div class="header-text">
                  <div class="header-title-row">
                    <h1 class="header-title">Sistema de Presentaciones</h1>
                    <span class="badge">v1.0</span>
                  </div>
                  <p class="header-subtitle">Panel de configuracion</p>
                </div>
              </div>
              <a href="/presentation" class="btn-presentation">
                <svg class="btn-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <polygon points="5 3 19 12 5 21 5 3"></polygon>
                </svg>
                Ir a la presentacion
                <svg class="btn-arrow" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <path d="M5 12h14"></path>
                  <path d="m12 5 7 7-7 7"></path>
                </svg>
              </a>
            </header>

            <hr class="separator">

            <!-- Section -->
            <div>
              <h2 class="section-label">Audio</h2>
              <p class="section-desc">Configura los filtros de audio del sistema de presentaciones.</p>
            </div>

            <!-- Settings -->
            <div class="settings-list">
              <!-- Volume Control -->
              <div class="volume-card {}">
                <div class="volume-icon-box">
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"></polygon>
                    <path d="M15.54 8.46a5 5 0 0 1 0 7.07"></path>
                    <path d="M19.07 4.93a10 10 0 0 1 0 14.14"></path>
                  </svg>
                </div>
                <div class="volume-text">
                  <span class="volume-title">Volumen</span>
                  <span class="volume-level">{}% - {}</span>
                </div>
                <div class="volume-controls">
                  <form method="POST" action="/settings/volume" style="display:inline;">
                    <input type="hidden" name="direction" value="down">
                    <button type="submit" class="volume-btn" aria-label="Bajar volumen" {}>-</button>
                  </form>
                  <form method="POST" action="/settings/volume" style="display:inline;">
                    <input type="hidden" name="direction" value="up">
                    <button type="submit" class="volume-btn" aria-label="Subir volumen" {}>+</button>
                  </form>
                </div>
              </div>

              <button type="button" class="limiter-card" id="limiterCard" aria-pressed="{}">
                <div class="limiter-icon-box">
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <path d="M2 16V8a2 2 0 0 1 2-2h2.93a2 2 0 0 0 1.66-.9l.82-1.2A2 2 0 0 1 11.07 3h1.86a2 2 0 0 1 1.66.9l.82 1.2A2 2 0 0 0 17.07 6H20a2 2 0 0 1 2 2v8"></path>
                    <path d="M2 17h20"></path>
                    <path d="M6 21h12"></path>
                    <circle cx="12" cy="11" r="3"></circle>
                  </svg>
                </div>
                <div class="limiter-text">
                  <span class="limiter-title">Limiter</span>
                  <span class="limiter-desc">Limita los picos de audio para proteger los altavoces</span>
                </div>
                <div class="toggle {}" id="limiterToggle" aria-label="Activar limiter de audio">
                  <div class="toggle-knob"></div>
                </div>
              </button>
            </div>

            <!-- Hidden form for submission -->
            <form method="POST" action="/settings/limiter" id="limiterForm" style="display:none;">
              <input type="checkbox" name="enable" {} id="limiterCheckbox">
            </form>
          </div>

          <script>
            (function () {{
              var card = document.getElementById('limiterCard');
              var toggle = document.getElementById('limiterToggle');
              var form = document.getElementById('limiterForm');
              var checkbox = document.getElementById('limiterCheckbox');
              var enabled = checkbox.checked;

              card.addEventListener('click', function () {{
                enabled = !enabled;
                toggle.classList.toggle('active', enabled);
                card.setAttribute('aria-pressed', String(enabled));
                checkbox.checked = enabled;
                card.disabled = true;
                form.submit();
              }});
              }})();
          </script>
        </body>
        </html>"#,
        if volume.is_none() { "disabled" } else { "" },
        volume.unwrap_or(0),
        if volume.is_some() {
            "Nivel de salida de audio"
        } else {
            "Control de volumen no disponible"
        },
        if volume.is_none() { "disabled" } else { "" },
        if volume.is_none() { "disabled" } else { "" },
        if limiter_on { "true" } else { "false" },
        if limiter_on { "active" } else { "" },
        if limiter_on { "checked" } else { "" }
    )
}

// Claude HUD — Example Custom Widget Script
// This script is called once per status line refresh.
// Available globals:
//   data  — object map with session fields (model_id, context_pct, cost_usd, ...)
//   theme — object map with color hex strings (accent, success, warning, ...)
//
// Your script must return a String — that string is rendered as your widget output.

let pct = data.context_pct;
let model = data.model_name;
let cost = data.cost_usd;

// Choose a color based on context usage
let color = if pct > 90.0 {
    theme.danger
} else if pct > 70.0 {
    theme.warning
} else {
    theme.success
};

// ANSI color helper (24-bit true color from hex)
fn hex_to_ansi_fg(hex, text) {
    // Strip '#' prefix
    let h = hex.sub_string(1, 7);
    let r = parse_int(h.sub_string(0, 2), 16);
    let g = parse_int(h.sub_string(2, 2), 16);
    let b = parse_int(h.sub_string(4, 2), 16);
    return `\x1b[38;2;${r};${g};${b}m${text}\x1b[0m`;
}

// Build a simple custom widget output
let output = `${hex_to_ansi_fg(theme.accent, "★")} ${model} `;
output += `${hex_to_ansi_fg(color, `${pct.to_int()}%`)} `;
output += `${hex_to_ansi_fg(theme.muted, `$${cost.to_fixed(2, 2)}`)}`;

// Return the rendered line
output

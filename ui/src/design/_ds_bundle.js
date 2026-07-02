/* @ds-bundle: {"format":3,"namespace":"OlympusDesignSystem_516a9b","components":[{"name":"Badge","sourcePath":"components/core/Badge.jsx"},{"name":"Button","sourcePath":"components/core/Button.jsx"},{"name":"IconButton","sourcePath":"components/core/IconButton.jsx"},{"name":"Spinner","sourcePath":"components/core/Spinner.jsx"},{"name":"Kbd","sourcePath":"components/core/Spinner.jsx"},{"name":"StatusDot","sourcePath":"components/core/StatusDot.jsx"},{"name":"Tag","sourcePath":"components/core/Tag.jsx"},{"name":"Avatar","sourcePath":"components/data/Avatar.jsx"},{"name":"Card","sourcePath":"components/data/Card.jsx"},{"name":"ProgressBar","sourcePath":"components/data/ProgressBar.jsx"},{"name":"Skeleton","sourcePath":"components/data/Skeleton.jsx"},{"name":"StatPill","sourcePath":"components/data/StatPill.jsx"},{"name":"Dialog","sourcePath":"components/feedback/Dialog.jsx"},{"name":"Toast","sourcePath":"components/feedback/Toast.jsx"},{"name":"Tooltip","sourcePath":"components/feedback/Tooltip.jsx"},{"name":"Checkbox","sourcePath":"components/forms/Checkbox.jsx"},{"name":"Input","sourcePath":"components/forms/Input.jsx"},{"name":"Radio","sourcePath":"components/forms/Radio.jsx"},{"name":"SearchInput","sourcePath":"components/forms/SearchInput.jsx"},{"name":"Select","sourcePath":"components/forms/Select.jsx"},{"name":"Switch","sourcePath":"components/forms/Switch.jsx"},{"name":"Textarea","sourcePath":"components/forms/Textarea.jsx"},{"name":"NavItem","sourcePath":"components/navigation/NavItem.jsx"},{"name":"Tabs","sourcePath":"components/navigation/Tabs.jsx"}],"sourceHashes":{"components/core/Badge.jsx":"6f7907ce6295","components/core/Button.jsx":"00438abfd3a7","components/core/IconButton.jsx":"ef40bb5b83a5","components/core/Spinner.jsx":"891e813845e7","components/core/StatusDot.jsx":"40effaa85cb9","components/core/Tag.jsx":"a154b71b4dcf","components/data/Avatar.jsx":"ae124a3ff171","components/data/Card.jsx":"955e4be61db4","components/data/ProgressBar.jsx":"7a6bd43cd464","components/data/Skeleton.jsx":"41a98cd3fb8c","components/data/StatPill.jsx":"4d43a6e262f0","components/feedback/Dialog.jsx":"02e13f94ee70","components/feedback/Toast.jsx":"d32633f6fe24","components/feedback/Tooltip.jsx":"ee4e98884c15","components/forms/Checkbox.jsx":"14096f49ef4e","components/forms/Input.jsx":"68d514203d52","components/forms/Radio.jsx":"e7f8ce91e21f","components/forms/SearchInput.jsx":"3e2b05fc2a63","components/forms/Select.jsx":"d2afa378dc7c","components/forms/Switch.jsx":"ac3d8e226bb8","components/forms/Textarea.jsx":"506206a91259","components/navigation/NavItem.jsx":"36cabc93972f","components/navigation/Tabs.jsx":"14b46ce7da36"},"inlinedExternals":[],"unexposedExports":[]} */

(() => {

const __ds_ns = (window.OlympusDesignSystem_516a9b = window.OlympusDesignSystem_516a9b || {});

const __ds_scope = {};

(__ds_ns.__errors = __ds_ns.__errors || []);

// components/core/Badge.jsx
try { (() => {
/**
 * Badge — mono, uppercase status pill. Kind maps to semantic color.
 * Use for statuses (running/blocked/done), counts, and mono tags.
 */
function Badge({
  kind,
  dot = false,
  className = "",
  children
}) {
  const map = {
    accent: "ol-badge-accent",
    ok: "ol-badge-ok",
    warn: "ol-badge-warn",
    err: "ol-badge-err",
    solid: "ol-badge-solid"
  };
  const cls = ["ol-badge", map[kind] || "", className].filter(Boolean).join(" ");
  const dotCls = {
    ok: "ol-dot-ok",
    warn: "ol-dot-warn",
    err: "ol-dot-err",
    accent: "ol-dot-accent"
  }[kind];
  return /*#__PURE__*/React.createElement("span", {
    className: cls
  }, dot && /*#__PURE__*/React.createElement("span", {
    className: `ol-dot ${dotCls || ""}`,
    style: {
      width: 5,
      height: 5,
      boxShadow: "none"
    }
  }), children);
}
Object.assign(__ds_scope, { Badge });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/Badge.jsx", error: String((e && e.message) || e) }); }

// components/core/Button.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/**
 * Button — the primary action primitive. Softly-squared, quiet outline default,
 * solid silver primary.
 * Variants map to intent: primary (accent fill), secondary (bordered), ghost
 * (bare), danger (destructive outline).
 */
function Button({
  variant = "primary",
  size = "md",
  block = false,
  icon,
  iconRight,
  disabled = false,
  type = "button",
  className = "",
  children,
  ...rest
}) {
  const cls = ["ol-btn", `ol-btn-${variant}`, size === "sm" && "ol-btn-sm", size === "lg" && "ol-btn-lg", block && "ol-btn-block", className].filter(Boolean).join(" ");
  return /*#__PURE__*/React.createElement("button", _extends({
    type: type,
    className: cls,
    disabled: disabled
  }, rest), icon, children, iconRight);
}
Object.assign(__ds_scope, { Button });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/Button.jsx", error: String((e && e.message) || e) }); }

// components/core/IconButton.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/** IconButton — square, icon-only control for toolbars and dense rows. */
function IconButton({
  size = "md",
  bordered = false,
  disabled = false,
  label,
  className = "",
  children,
  ...rest
}) {
  const cls = ["ol-iconbtn", size === "sm" && "ol-iconbtn-sm", bordered && "ol-iconbtn-bordered", className].filter(Boolean).join(" ");
  return /*#__PURE__*/React.createElement("button", _extends({
    type: "button",
    className: cls,
    disabled: disabled,
    "aria-label": label,
    title: label
  }, rest), children);
}
Object.assign(__ds_scope, { IconButton });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/IconButton.jsx", error: String((e && e.message) || e) }); }

// components/core/Spinner.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/** Spinner — indeterminate loading indicator. */
function Spinner({
  className = "",
  style,
  ...rest
}) {
  return /*#__PURE__*/React.createElement("span", _extends({
    className: `ol-spinner ${className}`,
    style: style,
    role: "status",
    "aria-label": "Loading"
  }, rest));
}

/** Kbd — a keyboard shortcut key cap. */
function Kbd({
  className = "",
  children
}) {
  return /*#__PURE__*/React.createElement("kbd", {
    className: `ol-kbd ${className}`
  }, children);
}
Object.assign(__ds_scope, { Spinner, Kbd });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/Spinner.jsx", error: String((e && e.message) || e) }); }

// components/core/StatusDot.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/** StatusDot — a small colored dot for liveness/health. `live` pulses. */
function StatusDot({
  status = "idle",
  live = false,
  className = "",
  ...rest
}) {
  const map = {
    ok: "ol-dot-ok",
    warn: "ol-dot-warn",
    err: "ol-dot-err",
    accent: "ol-dot-accent",
    idle: ""
  };
  const cls = ["ol-dot", live ? "ol-dot-live" : map[status] || "", className].filter(Boolean).join(" ");
  return /*#__PURE__*/React.createElement("span", _extends({
    className: cls
  }, rest));
}
Object.assign(__ds_scope, { StatusDot });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/StatusDot.jsx", error: String((e && e.message) || e) }); }

// components/core/Tag.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/**
 * Tag — a rounded pill. Static label, or interactive filter chip (`onClick`).
 * `dotColor` shows a leading channel/hue dot (e.g. session source).
 */
function Tag({
  active = false,
  dotColor,
  onClick,
  className = "",
  children,
  ...rest
}) {
  const interactive = typeof onClick === "function";
  const cls = ["ol-tag", interactive && "ol-tag-btn", active && "ol-tag-active", className].filter(Boolean).join(" ");
  const Comp = interactive ? "button" : "span";
  return /*#__PURE__*/React.createElement(Comp, _extends({
    type: interactive ? "button" : undefined,
    className: cls,
    onClick: onClick
  }, rest), dotColor && /*#__PURE__*/React.createElement("span", {
    className: "ol-tag-dot",
    style: {
      background: dotColor
    }
  }), children);
}
Object.assign(__ds_scope, { Tag });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/Tag.jsx", error: String((e && e.message) || e) }); }

// components/data/Avatar.jsx
try { (() => {
/** Avatar — circular initials or image. `agent` uses the accent identity tint. */
function Avatar({
  src,
  name = "",
  size = "md",
  agent = false,
  className = ""
}) {
  const initials = name.split(/[\s\-_]+/).filter(Boolean).slice(0, 2).map(w => w[0]).join("").toUpperCase();
  const cls = ["ol-avatar", size === "sm" && "ol-avatar-sm", size === "lg" && "ol-avatar-lg", agent && "ol-avatar-agent", className].filter(Boolean).join(" ");
  return /*#__PURE__*/React.createElement("span", {
    className: cls,
    title: name
  }, src ? /*#__PURE__*/React.createElement("img", {
    src: src,
    alt: name
  }) : initials || "?");
}
Object.assign(__ds_scope, { Avatar });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/data/Avatar.jsx", error: String((e && e.message) || e) }); }

// components/data/Card.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/**
 * Card — the flat surface primitive. Set `interactive` for hover-lift + focus,
 * `selected` for the accent edge, `accent` for a left accent rail.
 */
function Card({
  interactive = false,
  selected = false,
  accent = false,
  as,
  className = "",
  children,
  ...rest
}) {
  const cls = ["ol-card", interactive && "ol-card-interactive", selected && "ol-card-selected", accent && "ol-card-accent", className].filter(Boolean).join(" ");
  const Comp = as || (interactive ? "button" : "div");
  return /*#__PURE__*/React.createElement(Comp, _extends({
    type: Comp === "button" ? "button" : undefined,
    className: cls
  }, rest), children);
}
Object.assign(__ds_scope, { Card });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/data/Card.jsx", error: String((e && e.message) || e) }); }

// components/data/ProgressBar.jsx
try { (() => {
/** ProgressBar — determinate fill (0–100). `tone` colors the fill. */
function ProgressBar({
  value = 0,
  tone,
  className = ""
}) {
  const pct = Math.max(0, Math.min(100, value));
  return /*#__PURE__*/React.createElement("div", {
    className: `ol-bar ${className}`,
    role: "progressbar",
    "aria-valuenow": pct,
    "aria-valuemin": 0,
    "aria-valuemax": 100
  }, /*#__PURE__*/React.createElement("div", {
    className: `ol-bar-fill ${tone || ""}`,
    style: {
      width: `${pct}%`
    }
  }));
}
Object.assign(__ds_scope, { ProgressBar });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/data/ProgressBar.jsx", error: String((e && e.message) || e) }); }

// components/data/Skeleton.jsx
try { (() => {
/** Skeleton — shimmer placeholder line/block. Set width/height via props. */
function Skeleton({
  width = "100%",
  height = 11,
  className = "",
  style
}) {
  return /*#__PURE__*/React.createElement("span", {
    className: `ol-skel ${className}`,
    style: {
      display: "block",
      width,
      height,
      ...style
    }
  });
}
Object.assign(__ds_scope, { Skeleton });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/data/Skeleton.jsx", error: String((e && e.message) || e) }); }

// components/data/StatPill.jsx
try { (() => {
/** StatPill — a labeled metric block. Big mono value + uppercase label. */
function StatPill({
  label,
  value,
  delta,
  deltaDir,
  className = ""
}) {
  return /*#__PURE__*/React.createElement("div", {
    className: `ol-stat ${className}`
  }, /*#__PURE__*/React.createElement("span", {
    className: "ol-stat-value"
  }, value), /*#__PURE__*/React.createElement("span", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: "6px"
    }
  }, /*#__PURE__*/React.createElement("span", {
    className: "ol-stat-label"
  }, label), delta != null && /*#__PURE__*/React.createElement("span", {
    className: `ol-stat-delta ${deltaDir || ""}`
  }, delta)));
}
Object.assign(__ds_scope, { StatPill });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/data/StatPill.jsx", error: String((e && e.message) || e) }); }

// components/feedback/Dialog.jsx
try { (() => {
/**
 * Dialog — centered modal over a scrim. Controlled via `open`; `onClose` fires
 * on scrim click or Escape. Compose header/body/footer via props.
 */
function Dialog({
  open,
  onClose,
  title,
  children,
  footer,
  width
}) {
  React.useEffect(() => {
    if (!open) return;
    const onKey = e => {
      if (e.key === "Escape" && onClose) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);
  if (!open) return null;
  return /*#__PURE__*/React.createElement("div", {
    className: "ol-overlay",
    onClick: onClose,
    role: "presentation"
  }, /*#__PURE__*/React.createElement("div", {
    className: "ol-dialog",
    style: width ? {
      maxWidth: width
    } : undefined,
    role: "dialog",
    "aria-modal": "true",
    onClick: e => e.stopPropagation()
  }, title && /*#__PURE__*/React.createElement("div", {
    className: "ol-dialog-head"
  }, /*#__PURE__*/React.createElement("span", {
    className: "ol-dialog-title"
  }, title), /*#__PURE__*/React.createElement("button", {
    type: "button",
    className: "ol-iconbtn ol-iconbtn-sm",
    "aria-label": "Close",
    onClick: onClose
  }, /*#__PURE__*/React.createElement("svg", {
    width: "14",
    height: "14",
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "2"
  }, /*#__PURE__*/React.createElement("path", {
    d: "M18 6 6 18M6 6l12 12"
  })))), /*#__PURE__*/React.createElement("div", {
    className: "ol-dialog-body"
  }, children), footer && /*#__PURE__*/React.createElement("div", {
    className: "ol-dialog-foot"
  }, footer)));
}
Object.assign(__ds_scope, { Dialog });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/feedback/Dialog.jsx", error: String((e && e.message) || e) }); }

// components/feedback/Toast.jsx
try { (() => {
const ICONS = {
  ok: /*#__PURE__*/React.createElement("svg", {
    width: "15",
    height: "15",
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "2"
  }, /*#__PURE__*/React.createElement("path", {
    d: "M20 6 9 17l-5-5"
  })),
  warn: /*#__PURE__*/React.createElement("svg", {
    width: "15",
    height: "15",
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "2"
  }, /*#__PURE__*/React.createElement("path", {
    d: "M12 9v4M12 17h.01M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z"
  })),
  err: /*#__PURE__*/React.createElement("svg", {
    width: "15",
    height: "15",
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "2"
  }, /*#__PURE__*/React.createElement("circle", {
    cx: "12",
    cy: "12",
    r: "9"
  }), /*#__PURE__*/React.createElement("path", {
    d: "M12 8v5M12 16h.01"
  })),
  info: /*#__PURE__*/React.createElement("svg", {
    width: "15",
    height: "15",
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "2"
  }, /*#__PURE__*/React.createElement("circle", {
    cx: "12",
    cy: "12",
    r: "9"
  }), /*#__PURE__*/React.createElement("path", {
    d: "M12 11v5M12 8h.01"
  }))
};
const TONE_COLOR = {
  ok: "var(--ok)",
  warn: "var(--warn)",
  err: "var(--err)",
  info: "var(--accent)"
};

/** Toast — non-modal status notification. `tone` colors the icon. */
function Toast({
  tone = "info",
  title,
  message,
  onDismiss,
  className = ""
}) {
  const toneCls = {
    ok: "ol-toast-ok",
    warn: "ol-toast-warn",
    err: "ol-toast-err",
    info: ""
  }[tone];
  return /*#__PURE__*/React.createElement("div", {
    className: `ol-toast ${toneCls} ${className}`,
    role: "status"
  }, /*#__PURE__*/React.createElement("span", {
    className: "ol-toast-icon",
    style: {
      color: TONE_COLOR[tone]
    }
  }, ICONS[tone]), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      minWidth: 0
    }
  }, title && /*#__PURE__*/React.createElement("div", {
    className: "ol-toast-title"
  }, title), message && /*#__PURE__*/React.createElement("div", {
    className: "ol-toast-msg"
  }, message)), onDismiss && /*#__PURE__*/React.createElement("button", {
    type: "button",
    className: "ol-iconbtn ol-iconbtn-sm",
    "aria-label": "Dismiss",
    onClick: onDismiss
  }, /*#__PURE__*/React.createElement("svg", {
    width: "13",
    height: "13",
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "2"
  }, /*#__PURE__*/React.createElement("path", {
    d: "M18 6 6 18M6 6l12 12"
  }))));
}
Object.assign(__ds_scope, { Toast });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/feedback/Toast.jsx", error: String((e && e.message) || e) }); }

// components/feedback/Tooltip.jsx
try { (() => {
/** Tooltip — hover label above its trigger. Wraps a single child. */
function Tooltip({
  label,
  children,
  className = ""
}) {
  return /*#__PURE__*/React.createElement("span", {
    className: `ol-tooltip-wrap ${className}`
  }, children, /*#__PURE__*/React.createElement("span", {
    className: "ol-tooltip",
    role: "tooltip"
  }, label));
}
Object.assign(__ds_scope, { Tooltip });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/feedback/Tooltip.jsx", error: String((e && e.message) || e) }); }

// components/forms/Checkbox.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/** Checkbox — square check control with label. */
function Checkbox({
  label,
  checked,
  onChange,
  disabled = false,
  className = "",
  ...rest
}) {
  return /*#__PURE__*/React.createElement("label", {
    className: `ol-check ${className}`
  }, /*#__PURE__*/React.createElement("input", _extends({
    type: "checkbox",
    checked: checked,
    onChange: onChange,
    disabled: disabled
  }, rest)), /*#__PURE__*/React.createElement("span", {
    className: "ol-check-box"
  }, /*#__PURE__*/React.createElement("svg", {
    width: "11",
    height: "11",
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "3.5"
  }, /*#__PURE__*/React.createElement("path", {
    d: "M20 6 9 17l-5-5"
  }))), label && /*#__PURE__*/React.createElement("span", null, label));
}
Object.assign(__ds_scope, { Checkbox });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Checkbox.jsx", error: String((e && e.message) || e) }); }

// components/forms/Input.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/** Input — single-line text field with optional label. Mono variant for ids/data. */
function Input({
  label,
  mono = false,
  className = "",
  id,
  ...rest
}) {
  const inputId = id || (label ? `in-${Math.random().toString(36).slice(2, 8)}` : undefined);
  const input = /*#__PURE__*/React.createElement("input", _extends({
    id: inputId,
    className: ["ol-input", mono && "ol-input-mono", className].filter(Boolean).join(" ")
  }, rest));
  if (!label) return input;
  return /*#__PURE__*/React.createElement("label", {
    htmlFor: inputId,
    style: {
      display: "block"
    }
  }, /*#__PURE__*/React.createElement("span", {
    className: "ol-field-label"
  }, label), input);
}
Object.assign(__ds_scope, { Input });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Input.jsx", error: String((e && e.message) || e) }); }

// components/forms/Radio.jsx
try { (() => {
/** Radio — round single-select control with label. */
function Radio({
  label,
  name,
  value,
  checked,
  onChange,
  disabled = false,
  className = ""
}) {
  return /*#__PURE__*/React.createElement("label", {
    className: `ol-check ol-check-radio ${className}`
  }, /*#__PURE__*/React.createElement("input", {
    type: "radio",
    name: name,
    value: value,
    checked: checked,
    onChange: onChange,
    disabled: disabled
  }), /*#__PURE__*/React.createElement("span", {
    className: "ol-check-box"
  }), label && /*#__PURE__*/React.createElement("span", null, label));
}
Object.assign(__ds_scope, { Radio });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Radio.jsx", error: String((e && e.message) || e) }); }

// components/forms/SearchInput.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/** SearchInput — bordered input group with a leading magnifier icon. */
function SearchInput({
  placeholder = "Search…",
  value,
  onChange,
  className = "",
  ...rest
}) {
  return /*#__PURE__*/React.createElement("div", {
    className: `ol-inputgroup ${className}`
  }, /*#__PURE__*/React.createElement("svg", {
    width: "14",
    height: "14",
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "2"
  }, /*#__PURE__*/React.createElement("circle", {
    cx: "11",
    cy: "11",
    r: "8"
  }), /*#__PURE__*/React.createElement("path", {
    d: "m21 21-4.35-4.35"
  })), /*#__PURE__*/React.createElement("input", _extends({
    type: "text",
    placeholder: placeholder,
    value: value,
    onChange: onChange,
    "aria-label": placeholder
  }, rest)));
}
Object.assign(__ds_scope, { SearchInput });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/SearchInput.jsx", error: String((e && e.message) || e) }); }

// components/forms/Select.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/** Select — native dropdown, custom sharp chevron. Pass options or children. */
function Select({
  label,
  options,
  className = "",
  id,
  children,
  ...rest
}) {
  const selId = id || (label ? `sel-${Math.random().toString(36).slice(2, 8)}` : undefined);
  const sel = /*#__PURE__*/React.createElement("select", _extends({
    id: selId,
    className: `ol-select ${className}`
  }, rest), options ? options.map(o => {
    const value = typeof o === "string" ? o : o.value;
    const text = typeof o === "string" ? o : o.label;
    return /*#__PURE__*/React.createElement("option", {
      key: value,
      value: value
    }, text);
  }) : children);
  if (!label) return sel;
  return /*#__PURE__*/React.createElement("label", {
    htmlFor: selId,
    style: {
      display: "block"
    }
  }, /*#__PURE__*/React.createElement("span", {
    className: "ol-field-label"
  }, label), sel);
}
Object.assign(__ds_scope, { Select });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Select.jsx", error: String((e && e.message) || e) }); }

// components/forms/Switch.jsx
try { (() => {
/** Switch — on/off toggle. Use for binary settings (managed, archived). */
function Switch({
  label,
  checked,
  onChange,
  disabled = false,
  className = ""
}) {
  return /*#__PURE__*/React.createElement("label", {
    className: `ol-switch ${className}`,
    style: {
      display: "inline-flex",
      alignItems: "center",
      gap: "8px",
      cursor: "pointer"
    }
  }, /*#__PURE__*/React.createElement("input", {
    type: "checkbox",
    checked: checked,
    onChange: onChange,
    disabled: disabled
  }), /*#__PURE__*/React.createElement("span", {
    className: "ol-switch-track"
  }, /*#__PURE__*/React.createElement("span", {
    className: "ol-switch-thumb"
  })), label && /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: "var(--fs-13)",
      color: "var(--text-dim)"
    }
  }, label));
}
Object.assign(__ds_scope, { Switch });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Switch.jsx", error: String((e && e.message) || e) }); }

// components/forms/Textarea.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/** Textarea — multi-line input (composer, notes). Auto-label like Input. */
function Textarea({
  label,
  className = "",
  id,
  ...rest
}) {
  const taId = id || (label ? `ta-${Math.random().toString(36).slice(2, 8)}` : undefined);
  const ta = /*#__PURE__*/React.createElement("textarea", _extends({
    id: taId,
    className: `ol-textarea ${className}`
  }, rest));
  if (!label) return ta;
  return /*#__PURE__*/React.createElement("label", {
    htmlFor: taId,
    style: {
      display: "block"
    }
  }, /*#__PURE__*/React.createElement("span", {
    className: "ol-field-label"
  }, label), ta);
}
Object.assign(__ds_scope, { Textarea });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Textarea.jsx", error: String((e && e.message) || e) }); }

// components/navigation/NavItem.jsx
try { (() => {
/** NavItem — a sidebar navigation row. Icon + label, active accent rail. */
function NavItem({
  icon,
  label,
  active = false,
  badge,
  onClick,
  className = ""
}) {
  return /*#__PURE__*/React.createElement("button", {
    type: "button",
    className: `ol-nav ${active ? "ol-nav-active" : ""} ${className}`,
    onClick: onClick,
    "aria-current": active ? "page" : undefined
  }, icon, /*#__PURE__*/React.createElement("span", null, label), badge != null && /*#__PURE__*/React.createElement("span", {
    className: "ol-nav-badge"
  }, badge));
}
Object.assign(__ds_scope, { NavItem });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/navigation/NavItem.jsx", error: String((e && e.message) || e) }); }

// components/navigation/Tabs.jsx
try { (() => {
/**
 * Tabs — underline tab bar. Controlled: pass `value` + `onChange` and an array
 * of { value, label, badge } items.
 */
function Tabs({
  items = [],
  value,
  onChange,
  className = ""
}) {
  return /*#__PURE__*/React.createElement("div", {
    className: `ol-tabs ${className}`,
    role: "tablist"
  }, items.map(it => /*#__PURE__*/React.createElement("button", {
    type: "button",
    key: it.value,
    role: "tab",
    "aria-selected": value === it.value,
    className: `ol-tab ${value === it.value ? "ol-tab-active" : ""}`,
    onClick: () => onChange && onChange(it.value)
  }, it.label, it.badge != null && /*#__PURE__*/React.createElement("span", {
    style: {
      marginLeft: 6,
      fontFamily: "var(--font-mono)",
      fontSize: "var(--fs-11)",
      color: "var(--text-faint)"
    }
  }, it.badge))));
}
Object.assign(__ds_scope, { Tabs });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/navigation/Tabs.jsx", error: String((e && e.message) || e) }); }

__ds_ns.Badge = __ds_scope.Badge;

__ds_ns.Button = __ds_scope.Button;

__ds_ns.IconButton = __ds_scope.IconButton;

__ds_ns.Spinner = __ds_scope.Spinner;

__ds_ns.Kbd = __ds_scope.Kbd;

__ds_ns.StatusDot = __ds_scope.StatusDot;

__ds_ns.Tag = __ds_scope.Tag;

__ds_ns.Avatar = __ds_scope.Avatar;

__ds_ns.Card = __ds_scope.Card;

__ds_ns.ProgressBar = __ds_scope.ProgressBar;

__ds_ns.Skeleton = __ds_scope.Skeleton;

__ds_ns.StatPill = __ds_scope.StatPill;

__ds_ns.Dialog = __ds_scope.Dialog;

__ds_ns.Toast = __ds_scope.Toast;

__ds_ns.Tooltip = __ds_scope.Tooltip;

__ds_ns.Checkbox = __ds_scope.Checkbox;

__ds_ns.Input = __ds_scope.Input;

__ds_ns.Radio = __ds_scope.Radio;

__ds_ns.SearchInput = __ds_scope.SearchInput;

__ds_ns.Select = __ds_scope.Select;

__ds_ns.Switch = __ds_scope.Switch;

__ds_ns.Textarea = __ds_scope.Textarea;

__ds_ns.NavItem = __ds_scope.NavItem;

__ds_ns.Tabs = __ds_scope.Tabs;

})();

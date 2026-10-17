import AppKit
import Carbon.HIToolbox
import ApplicationServices
import Security

// JSON-lines protocol over stdio.
// In : {"id":1,"cmd":"read","excluded":["bundle.id"]}
//      {"id":2,"cmd":"write","text":"...","pid":123,"method":"ax|clipboard"}
//      {"id":3,"cmd":"status"} {"cmd":"prompt_ax"} {"cmd":"set_hotkey","key":"G","mods":["alt","cmd"]}
//      {"cmd":"keychain_get|keychain_set|keychain_delete","service":"..","account":"..","value":".."}
// Out: {"id":1,"ok":true,...} | {"id":1,"ok":false,"error":"..."} | {"event":"hotkey"}

let out = FileHandle.standardOutput
let outLock = NSLock()

func emit(_ obj: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: obj, options: [.withoutEscapingSlashes]) else { return }
    outLock.lock(); defer { outLock.unlock() }
    out.write(data); out.write("\n".data(using: .utf8)!)
}

func reply(_ id: Any?, _ ok: Bool, _ extra: [String: Any] = [:]) {
    var o = extra
    if let id = id { o["id"] = id }
    o["ok"] = ok
    emit(o)
}

// MARK: - Accessibility

func axTrusted(prompt: Bool = false) -> Bool {
    let opts = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: prompt] as CFDictionary
    return AXIsProcessTrustedWithOptions(opts)
}

func axString(_ el: AXUIElement, _ attr: String) -> String? {
    var v: CFTypeRef?
    guard AXUIElementCopyAttributeValue(el, attr as CFString, &v) == .success else { return nil }
    return v as? String
}

/// Focused element of the frontmost app (or of `pid`), not system-wide: our own popover must never be mistaken for the target.
var axEnabledPids = Set<pid_t>()

func focusedElement(pid: pid_t? = nil) -> AXUIElement? {
    guard let p = pid ?? NSWorkspace.shared.frontmostApplication?.processIdentifier else { return nil }
    let appEl = AXUIElementCreateApplication(p)
    if !axEnabledPids.contains(p) { // Chromium/Electron apps (Claude, Slack, Chrome) only build their AX tree on request
        axEnabledPids.insert(p)
        AXUIElementSetAttributeValue(appEl, "AXManualAccessibility" as CFString, kCFBooleanTrue)
        AXUIElementSetAttributeValue(appEl, "AXEnhancedUserInterface" as CFString, kCFBooleanTrue)
        sleepMs(120)
    }
    var v: CFTypeRef?
    guard AXUIElementCopyAttributeValue(appEl, kAXFocusedUIElementAttribute as CFString, &v) == .success,
          let v = v, CFGetTypeID(v) == AXUIElementGetTypeID() else { return nil }
    return (v as! AXUIElement)
}

func isSecure(_ el: AXUIElement) -> Bool {
    if axString(el, kAXSubroleAttribute as String) == "AXSecureTextField" { return true }
    if axString(el, kAXRoleAttribute as String) == "AXSecureTextField" { return true }
    return false
}

// MARK: - Key synthesis / clipboard

func postKey(_ code: CGKeyCode, flags: CGEventFlags) {
    let src = CGEventSource(stateID: .combinedSessionState)
    let down = CGEvent(keyboardEventSource: src, virtualKey: code, keyDown: true)
    let up = CGEvent(keyboardEventSource: src, virtualKey: code, keyDown: false)
    down?.flags = flags; up?.flags = flags
    down?.post(tap: .cgSessionEventTap)
    up?.post(tap: .cgSessionEventTap)
}

struct PasteboardSnapshot {
    let items: [[NSPasteboard.PasteboardType: Data]]
    init(_ pb: NSPasteboard) {
        items = (pb.pasteboardItems ?? []).map { item in
            var d: [NSPasteboard.PasteboardType: Data] = [:]
            for t in item.types { if let data = item.data(forType: t) { d[t] = data } }
            return d
        }
    }
    func restore(_ pb: NSPasteboard) {
        pb.clearContents()
        let objs: [NSPasteboardItem] = items.map { d in
            let it = NSPasteboardItem()
            for (t, data) in d { it.setData(data, forType: t) }
            return it
        }
        if !objs.isEmpty { pb.writeObjects(objs) }
    }
}

func sleepMs(_ ms: Int) { Thread.sleep(forTimeInterval: Double(ms) / 1000) }

func clipboardCopySelection() -> String? {
    let pb = NSPasteboard.general
    let snap = PasteboardSnapshot(pb)
    let before = pb.changeCount
    postKey(CGKeyCode(kVK_ANSI_C), flags: .maskCommand)
    var text: String? = nil
    for _ in 0..<20 {
        sleepMs(25)
        if pb.changeCount != before { text = pb.string(forType: .string); break }
    }
    snap.restore(pb)
    return text
}

func clipboardPaste(_ text: String) {
    let pb = NSPasteboard.general
    let snap = PasteboardSnapshot(pb)
    pb.clearContents()
    pb.setString(text, forType: .string)
    sleepMs(150) // let the accept hotkey's modifiers release before pasting
    postKey(CGKeyCode(kVK_ANSI_V), flags: .maskCommand)
    sleepMs(500)
    snap.restore(pb)
}

// MARK: - Commands

func handleRead(_ id: Any?, _ msg: [String: Any]) {
    guard axTrusted() else { return reply(id, false, ["error": "accessibility_not_trusted"]) }
    guard let app = NSWorkspace.shared.frontmostApplication else { return reply(id, false, ["error": "no_frontmost_app"]) }
    let bundle = app.bundleIdentifier ?? ""
    let appInfo: [String: Any] = ["bundleId": bundle, "appName": app.localizedName ?? "", "pid": Int(app.processIdentifier)]
    let excluded = (msg["excluded"] as? [String]) ?? []
    if excluded.contains(bundle) {
        return reply(id, false, appInfo.merging(["error": "excluded_app"]) { a, _ in a })
    }
    let el = focusedElement()
    if let el = el, isSecure(el) {
        return reply(id, false, appInfo.merging(["error": "secure_field"]) { a, _ in a })
    }
    if let el = el, let sel = axString(el, kAXSelectedTextAttribute as String), !sel.isEmpty {
        return reply(id, true, appInfo.merging(["text": sel, "method": "ax"]) { a, _ in a })
    }
    if (msg["selectAll"] as? Bool) == true, let role = el.flatMap({ axString($0, kAXRoleAttribute as String) }), ["AXTextField", "AXTextArea"].contains(role) {
        postKey(0, flags: .maskCommand) // Cmd+A: no selection, so check the whole field
        sleepMs(80)
        if let el = el, let sel = axString(el, kAXSelectedTextAttribute as String), !sel.isEmpty {
            return reply(id, true, appInfo.merging(["text": sel, "method": "ax"]) { a, _ in a })
        }
    }
    // Fallback: simulate Cmd+C.
    if let text = clipboardCopySelection(), !text.isEmpty {
        return reply(id, true, appInfo.merging(["text": text, "method": "clipboard"]) { a, _ in a })
    }
    reply(id, false, appInfo.merging(["error": "no_selection"]) { a, _ in a })
}

/// Where to anchor the typing badge: caret bounds if the app reports them, else the focused text element's frame.
func handleCaret(_ id: Any?, _ msg: [String: Any]) {
    guard axTrusted(), let app = NSWorkspace.shared.frontmostApplication else { return reply(id, true, ["visible": false]) }
    let bundle = app.bundleIdentifier ?? ""
    if ((msg["excluded"] as? [String]) ?? []).contains(bundle) { return reply(id, true, ["visible": false]) }
    guard let el = focusedElement(), !isSecure(el) else { return reply(id, true, ["visible": false]) }
    let role = axString(el, kAXRoleAttribute as String) ?? ""
    var rangeRef: CFTypeRef?
    let hasRange = AXUIElementCopyAttributeValue(el, kAXSelectedTextRangeAttribute as CFString, &rangeRef) == .success
    guard hasRange || ["AXTextField", "AXTextArea", "AXComboBox"].contains(role) else { return reply(id, true, ["visible": false]) }
    var rect = CGRect.zero
    var precise = false
    var selLen = 0
    var cf = CFRange(location: 0, length: 0)
    if hasRange, let r = rangeRef, AXValueGetValue(r as! AXValue, .cfRange, &cf) { selLen = cf.length }
    if hasRange, let r = rangeRef {
        var b: CFTypeRef?
        if AXUIElementCopyParameterizedAttributeValue(el, kAXBoundsForRangeParameterizedAttribute as CFString, r, &b) == .success,
           let b = b, CFGetTypeID(b) == AXValueGetTypeID(),
           AXValueGetValue(b as! AXValue, .cgRect, &rect), rect.height > 0, rect.height < 200, rect.width > 0 { precise = true }
    }
    // Chromium reports 0×0 or the whole window for a range: measure the first and last selected characters instead.
    if !precise, hasRange, msg["deep"] as? Bool == true, let value = axString(el, kAXValueAttribute as String), value.utf16.count <= 20000 {
        let n = value.utf16.count
        let first = min(max(cf.length > 0 ? cf.location : cf.location - 1, 0), max(n - 1, 0))
        let last = min(max(cf.location + cf.length - 1, first), max(n - 1, 0))
        var leaves: [(el: AXUIElement, start: Int, len: Int)]? = nil
        if n > 0, let a = rangeRect(el, value, &leaves, first, 1) {
            rect = rangeRect(el, value, &leaves, last, 1).map { a.union($0) } ?? a
            precise = true
        }
    }
    if !precise {
        var pv: CFTypeRef?, sv: CFTypeRef?
        var pt = CGPoint.zero, sz = CGSize.zero
        guard AXUIElementCopyAttributeValue(el, kAXPositionAttribute as CFString, &pv) == .success,
              AXUIElementCopyAttributeValue(el, kAXSizeAttribute as CFString, &sv) == .success,
              AXValueGetValue(pv as! AXValue, .cgPoint, &pt), AXValueGetValue(sv as! AXValue, .cgSize, &sz) else {
            return reply(id, true, ["visible": false])
        }
        rect = CGRect(origin: pt, size: sz)
    }
    reply(id, true, ["visible": true, "precise": precise, "selLen": selLen, "x": rect.origin.x, "y": rect.origin.y, "w": rect.size.width, "h": rect.size.height, "bundleId": bundle])
}

func cfRangeValue(_ loc: Int, _ len: Int) -> AXValue? {
    var cf = CFRange(location: loc, length: len)
    return AXValueCreate(.cfRange, &cf)
}

/// Bounds of a range that sits on one text line; nil when the app has none (or reports the whole field/window).
func lineRect(_ el: AXUIElement, _ loc: Int, _ len: Int) -> CGRect? {
    guard let v = cfRangeValue(loc, len) else { return nil }
    var b: CFTypeRef?
    var rect = CGRect.zero
    guard AXUIElementCopyParameterizedAttributeValue(el, kAXBoundsForRangeParameterizedAttribute as CFString, v, &b) == .success,
          let b = b, CFGetTypeID(b) == AXValueGetTypeID(), AXValueGetValue(b as! AXValue, .cgRect, &rect),
          rect.height > 0, rect.height < 60, rect.width > 0 else { return nil }
    return rect
}

/// `lineRect`, falling back to the AXStaticText leaves: Chromium contenteditable answers only there, with leaf-local offsets.
func rangeRect(_ el: AXUIElement, _ value: String, _ leaves: inout [(el: AXUIElement, start: Int, len: Int)]?, _ loc: Int, _ len: Int) -> CGRect? {
    if let r = lineRect(el, loc, len) { return r }
    if leaves == nil { leaves = textLeaves(el, value) }
    guard let lf = leaves!.first(where: { loc >= $0.start && loc < $0.start + $0.len }) else { return nil }
    return lineRect(lf.el, loc - lf.start, min(len, lf.start + lf.len - loc))
}

/// AXStaticText descendants in document order, each located in `value` (UTF-16 offsets); paragraph breaks exist only in `value`.
func textLeaves(_ root: AXUIElement, _ value: String) -> [(el: AXUIElement, start: Int, len: Int)] {
    let full = value as NSString
    var out: [(el: AXUIElement, start: Int, len: Int)] = []
    var cursor = 0
    var visited = 0
    func walk(_ e: AXUIElement) {
        visited += 1
        if visited > 3000 { return }
        if axString(e, kAXRoleAttribute as String) == "AXStaticText", let t = axString(e, kAXValueAttribute as String), !t.isEmpty {
            let r = full.range(of: t, options: [], range: NSRange(location: cursor, length: full.length - cursor))
            if r.location != NSNotFound { out.append((e, r.location, r.length)); cursor = r.location + r.length }
            return
        }
        var kids: CFTypeRef?
        guard AXUIElementCopyAttributeValue(e, kAXChildrenAttribute as CFString, &kids) == .success, let list = kids as? [AXUIElement] else { return }
        for k in list { walk(k) }
    }
    walk(root)
    return out
}

/// Text, caret and per-range bounds of the focused field, for auto-check and underlines.
func handleSnapshot(_ id: Any?, _ msg: [String: Any]) {
    guard axTrusted(), let app = NSWorkspace.shared.frontmostApplication else { return reply(id, true, ["visible": false]) }
    let bundle = app.bundleIdentifier ?? ""
    if ((msg["excluded"] as? [String]) ?? []).contains(bundle) { return reply(id, true, ["visible": false]) }
    guard let el = focusedElement() else { return reply(id, true, ["visible": false, "reason": "no_focus"]) }
    let role = axString(el, kAXRoleAttribute as String) ?? ""
    guard !isSecure(el), ["AXTextField", "AXTextArea", "AXComboBox", "AXWebArea"].contains(role) else {
        return reply(id, true, ["visible": false, "reason": "role:\(role)"])
    }
    guard let value = axString(el, kAXValueAttribute as String), value.utf16.count <= 20000 else {
        return reply(id, true, ["visible": false, "reason": "no_value:\(role)"])
    }
    var rangeRef: CFTypeRef?
    var cf = CFRange(location: 0, length: 0)
    guard AXUIElementCopyAttributeValue(el, kAXSelectedTextRangeAttribute as CFString, &rangeRef) == .success,
          let rr = rangeRef, AXValueGetValue(rr as! AXValue, .cfRange, &cf) else { return reply(id, true, ["visible": false, "reason": "no_range:\(role)"]) }
    var bounds: [Any] = []
    var leaves: [(el: AXUIElement, start: Int, len: Int)]? = nil
    for r in (msg["ranges"] as? [[String: Any]]) ?? [] {
        guard let loc = r["loc"] as? Int, let len = r["len"] as? Int else { bounds.append(NSNull()); continue }
        if let rect = rangeRect(el, value, &leaves, loc, len) { bounds.append(["x": rect.origin.x, "y": rect.origin.y, "w": rect.size.width, "h": rect.size.height]) }
        else { bounds.append(NSNull()) }
    }
    reply(id, true, ["visible": true, "text": value, "caret": cf.location, "selLen": cf.length, "pid": Int(app.processIdentifier),
                     "bundleId": bundle, "appName": app.localizedName ?? "", "bounds": bounds])
}

/// Replace one range of the focused field, only if it still holds `expect` (the user may have kept typing).
func handleReplaceRange(_ id: Any?, _ msg: [String: Any]) {
    guard axTrusted() else { return reply(id, false, ["error": "accessibility_not_trusted"]) }
    guard let loc = msg["loc"] as? Int, let len = msg["len"] as? Int, let text = msg["text"] as? String, let expect = msg["expect"] as? String else {
        return reply(id, false, ["error": "bad_args"])
    }
    if let pid = msg["pid"] as? Int, NSWorkspace.shared.frontmostApplication?.processIdentifier != pid_t(pid) {
        return reply(id, false, ["error": "focus_changed"])
    }
    guard let el = focusedElement(), !isSecure(el), let value = axString(el, kAXValueAttribute as String) else {
        return reply(id, false, ["error": "no_field"])
    }
    let u = Array(value.utf16)
    guard loc >= 0, loc + len <= u.count, String(utf16CodeUnits: Array(u[loc..<(loc + len)]), count: len) == expect else {
        return reply(id, false, ["error": "stale"])
    }
    var oldCaret = CFRange(location: loc + len, length: 0)
    var rr: CFTypeRef?
    if AXUIElementCopyAttributeValue(el, kAXSelectedTextRangeAttribute as CFString, &rr) == .success, let r = rr { _ = AXValueGetValue(r as! AXValue, .cfRange, &oldCaret) }
    guard let sel = cfRangeValue(loc, len),
          AXUIElementSetAttributeValue(el, kAXSelectedTextRangeAttribute as CFString, sel) == .success,
          AXUIElementSetAttributeValue(el, kAXSelectedTextAttribute as CFString, text as CFTypeRef) == .success else {
        return reply(id, false, ["error": "range_unsupported"])
    }
    let delta = text.utf16.count - len
    let newCaret = oldCaret.location >= loc + len ? oldCaret.location + delta : loc + text.utf16.count
    if let c = cfRangeValue(max(0, newCaret), 0) { AXUIElementSetAttributeValue(el, kAXSelectedTextRangeAttribute as CFString, c) }
    reply(id, true)
}

func handleWrite(_ id: Any?, _ msg: [String: Any]) {
    guard axTrusted() else { return reply(id, false, ["error": "accessibility_not_trusted"]) }
    guard let text = msg["text"] as? String else { return reply(id, false, ["error": "missing_text"]) }
    if let pid = msg["pid"] as? Int,
       NSWorkspace.shared.frontmostApplication?.processIdentifier != pid_t(pid) {
        return reply(id, false, ["error": "focus_changed"])
    }
    let method = (msg["method"] as? String) ?? "ax"
    if method == "ax", let el = focusedElement(), !isSecure(el) {
        var settable: DarwinBoolean = false
        if AXUIElementIsAttributeSettable(el, kAXSelectedTextAttribute as CFString, &settable) == .success, settable.boolValue {
            let r = AXUIElementSetAttributeValue(el, kAXSelectedTextAttribute as CFString, text as CFTypeRef)
            if r == .success {
                // Some apps report success but ignore the write; verify selection changed.
                sleepMs(60)
                let now = axString(el, kAXSelectedTextAttribute as String)
                if now == text || now == nil || now == "" {
                    return reply(id, true, ["method": "ax"])
                }
            }
        }
    }
    if let el = focusedElement(), isSecure(el) { return reply(id, false, ["error": "secure_field"]) }
    clipboardPaste(text)
    reply(id, true, ["method": "clipboard"])
}

// MARK: - Keychain

func keychainQuery(_ msg: [String: Any]) -> [String: Any] {
    [kSecClass as String: kSecClassGenericPassword,
     kSecAttrService as String: (msg["service"] as? String) ?? "Proofy",
     kSecAttrAccount as String: (msg["account"] as? String) ?? "api-key"]
}

func handleKeychain(_ id: Any?, _ cmd: String, _ msg: [String: Any]) {
    var q = keychainQuery(msg)
    switch cmd {
    case "keychain_get":
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var res: CFTypeRef?
        let s = SecItemCopyMatching(q as CFDictionary, &res)
        if s == errSecSuccess, let d = res as? Data, let str = String(data: d, encoding: .utf8) {
            reply(id, true, ["value": str])
        } else if s == errSecItemNotFound {
            reply(id, true, ["value": NSNull()])
        } else { reply(id, false, ["error": "keychain_status_\(s)"]) }
    case "keychain_set":
        guard let v = msg["value"] as? String else { return reply(id, false, ["error": "missing_value"]) }
        SecItemDelete(q as CFDictionary)
        q[kSecValueData as String] = v.data(using: .utf8)!
        let s = SecItemAdd(q as CFDictionary, nil)
        s == errSecSuccess ? reply(id, true) : reply(id, false, ["error": "keychain_status_\(s)"])
    default:
        let s = SecItemDelete(q as CFDictionary)
        (s == errSecSuccess || s == errSecItemNotFound) ? reply(id, true) : reply(id, false, ["error": "keychain_status_\(s)"])
    }
}

// MARK: - Hotkey

var hotKeyRefs: [UInt32: EventHotKeyRef] = [:]
var handlerInstalled = false

let keyCodes: [String: Int] = [
    "A": kVK_ANSI_A, "B": kVK_ANSI_B, "C": kVK_ANSI_C, "D": kVK_ANSI_D, "E": kVK_ANSI_E, "F": kVK_ANSI_F,
    "G": kVK_ANSI_G, "H": kVK_ANSI_H, "I": kVK_ANSI_I, "J": kVK_ANSI_J, "K": kVK_ANSI_K, "L": kVK_ANSI_L,
    "M": kVK_ANSI_M, "N": kVK_ANSI_N, "O": kVK_ANSI_O, "P": kVK_ANSI_P, "Q": kVK_ANSI_Q, "R": kVK_ANSI_R,
    "S": kVK_ANSI_S, "T": kVK_ANSI_T, "U": kVK_ANSI_U, "V": kVK_ANSI_V, "W": kVK_ANSI_W, "X": kVK_ANSI_X,
    "Y": kVK_ANSI_Y, "Z": kVK_ANSI_Z,
]

func setHotkey(key: String, mods: [String], slot: UInt32 = 1) -> String? {
    guard let code = keyCodes[key.uppercased()] else { return "unknown_key" }
    var m: UInt32 = 0
    for x in mods {
        switch x.lowercased() {
        case "cmd": m |= UInt32(cmdKey)
        case "alt", "option": m |= UInt32(optionKey)
        case "ctrl", "control": m |= UInt32(controlKey)
        case "shift": m |= UInt32(shiftKey)
        default: return "unknown_modifier"
        }
    }
    if m == 0 { return "modifier_required" }
    if !handlerInstalled {
        var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(GetApplicationEventTarget(), { _, event, _ in
            var hk = EventHotKeyID()
            GetEventParameter(event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID), nil, MemoryLayout<EventHotKeyID>.size, nil, &hk)
            emit(["event": hk.id == 2 ? "hotkey2" : "hotkey"])
            return noErr
        }, 1, &spec, nil, nil)
        handlerInstalled = true
    }
    if let r = hotKeyRefs[slot] { UnregisterEventHotKey(r); hotKeyRefs[slot] = nil }
    let hkID = EventHotKeyID(signature: OSType(0x50524659), id: slot)
    var ref: EventHotKeyRef?
    let s = RegisterEventHotKey(UInt32(code), m, hkID, GetApplicationEventTarget(), 0, &ref)
    if s == noErr { hotKeyRefs[slot] = ref }
    return s == noErr ? nil : "register_failed_\(s)"
}

// MARK: - Dispatch

func dispatch(_ msg: [String: Any]) {
    let id = msg["id"]
    switch msg["cmd"] as? String ?? "" {
    case "status":
        reply(id, true, ["trusted": axTrusted(), "version": 1])
    case "prompt_ax":
        reply(id, true, ["trusted": axTrusted(prompt: true)])
    case "set_hotkey":
        let err = DispatchQueue.main.sync {
            setHotkey(key: (msg["key"] as? String) ?? "", mods: (msg["mods"] as? [String]) ?? [], slot: UInt32((msg["slot"] as? Int) ?? 1))
        }
        err == nil ? reply(id, true) : reply(id, false, ["error": err!])
    case "read": handleRead(id, msg)
    case "write": handleWrite(id, msg)
    case "caret": handleCaret(id, msg)
    case "snapshot": handleSnapshot(id, msg)
    case "replace_range": handleReplaceRange(id, msg)
    case let c where c.hasPrefix("keychain_"): handleKeychain(id, c, msg)
    default: reply(id, false, ["error": "unknown_command"])
    }
}

setvbuf(stdout, nil, _IOLBF, 0)
let app = NSApplication.shared
app.setActivationPolicy(.prohibited)

let worker = DispatchQueue(label: "helper.worker")
Thread.detachNewThread {
    while let line = readLine() {
        guard let data = line.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { continue }
        // Serial: read/write must not interleave.
        worker.sync { dispatch(obj) }
    }
    exit(0) // stdin closed: parent gone
}

emit(["event": "ready"])
app.run()

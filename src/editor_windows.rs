//! Native Win32/GDI bootstrap UI. All unsafe code is confined to OS handles,
//! live UTF-16 buffers and a window-owned RefCell released at WM_NCDESTROY.
use g0::{
    editor::{DebugSession, EditorLayout, GraphEditor, OPERATION_NAMES},
    execution::TraceEvent,
    gir::*,
};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    rc::Rc,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Controls::Dialogs::*, WindowsAndMessaging::*},
};

const NEW: usize = 101;
const OPEN: usize = 102;
const SAVE: usize = 103;
const SAVE_AS: usize = 104;
const UNDO: usize = 201;
const REDO: usize = 202;
const ADD: usize = 203;
const PLUS: usize = 204;
const DELETE: usize = 205;
const NEXT_GRAPH: usize = 206;
const CHECK: usize = 301;
const RUN: usize = 302;
const TRACE: usize = 303;
const APPLY: usize = 401;
const APPLY_TYPE: usize = 402;
const OUTPUT_TYPE: usize = 403;
const ADD_GRAPH: usize = 404;
const PROPERTIES: usize = 405;
const DUPLICATE: usize = 406;
const SUBGRAPH: usize = 407;
const NODE_FORM: usize = 408;
const OPERATION_CHOICE: usize = 409;
const FORM_APPLY: usize = 410;
const SCHEMA_FORM: usize = 411;
const SCHEMA_VIEW: usize = 412;
const FORM_CANCEL: usize = 413;
const EDIT_NODE_FORM: usize = 414;
const GRAPH_FORM: usize = 415;
#[derive(Clone, Copy)]
enum FormMode {
    Node,
    Schema,
    Graph,
    EditNode(NodeId),
}
const TRACE_PREVIOUS: usize = 304;
const DEBUG_START: usize = 305;
const DEBUG_STEP: usize = 306;
const DEBUG_CONTINUE: usize = 307;
const DEBUG_PAUSE: usize = 308;
const DEBUG_STOP: usize = 309;
const DEBUG_BREAK: usize = 310;
const TOOLBOX: usize = 500;
fn toolbox() -> Vec<(&'static str, Operation)> {
    vec![
        ("Entero", Operation::Const(Literal::Integer(42))),
        ("Booleano", Operation::Const(Literal::Bool(true))),
        ("Texto", Operation::Const(Literal::Text(String::new()))),
        ("Bytes hex", Operation::Const(Literal::Bytes(vec![]))),
        ("Suma", Operation::Add),
        ("Resta", Operation::Sub),
        ("Producto", Operation::Mul),
        ("División", Operation::Div),
        ("Resto", Operation::Rem),
        ("Igual", Operation::Eq),
        ("Menor", Operation::Lt),
        ("Menor o igual", Operation::Le),
        ("Mayor", Operation::Gt),
        ("Mayor o igual", Operation::Ge),
        ("And", Operation::And),
        ("Or", Operation::Or),
        ("Xor", Operation::Xor),
        ("Not", Operation::Not),
        ("Concatenar texto", Operation::TextConcat),
        ("Concatenar bytes", Operation::BytesConcat),
        ("Codificar UTF8", Operation::EncodeUtf8),
        ("Decodificar UTF8", Operation::DecodeUtf8),
        ("Formatear entero", Operation::FormatInteger),
        ("Conversión checked", Operation::ConvertChecked),
        ("Array entero (2)", Operation::MakeArray),
        ("Index bytes", Operation::Index),
        ("Longitud bytes", Operation::Length),
        ("Some entero", Operation::Some),
        ("None entero", Operation::None),
        ("Ok entero", Operation::Ok),
        ("Err texto", Operation::Err),
        ("UnwrapOr entero", Operation::UnwrapOr),
        ("Range", Operation::Range),
        ("Concatenar arrays", Operation::ArrayConcat),
        ("Slice bytes", Operation::BytesSlice),
        ("Bytes desde array", Operation::BytesFromArray),
        ("Unir textos", Operation::TextJoin),
    ]
}
#[derive(Clone, Copy)]
struct Point {
    x: i32,
    y: i32,
}
enum Endpoint {
    Source(SourceEndpoint),
    Target(TargetEndpoint),
}
struct Hit {
    point: Point,
    endpoint: Endpoint,
}
struct Window {
    editor: GraphEditor,
    path: Option<PathBuf>,
    selected: Option<NodeId>,
    pending: Option<SourceEndpoint>,
    layout: EditorLayout,
    scroll: i32,
    drag: Option<(NodeId, Point)>,
    ports: Vec<Hit>,
    width: i32,
    height: i32,
    diagnostic: HWND,
    literal: HWND,
    apply: HWND,
    operation_choice: HWND,
    form_apply: HWND,
    schema_form: Option<FormMode>,
    trace: Vec<TraceEvent>,
    trace_index: usize,
    debugger: Option<DebugSession>,
    breakpoints: BTreeSet<(String, NodeId)>,
    status: String,
}
impl Window {
    fn new(editor: GraphEditor, path: Option<PathBuf>) -> Self {
        let layout = path.as_deref().map(load_layout).unwrap_or_default();
        Self { editor,path,selected: None,pending: None,layout,scroll: 0,drag: None,ports: Vec::new(),width: 1100,height: 700,diagnostic: null_mut(),literal: null_mut(),apply: null_mut(),operation_choice:null_mut(),form_apply:null_mut(),schema_form:None,trace: Vec::new(),trace_index: 0,debugger:None,breakpoints:BTreeSet::new(),status: "Selecciona una salida y después una entrada para conectar. La validación comprueba los tipos.".into() }
    }
    fn node_rect(&self, node: NodeId, index: usize) -> RECT {
        let p = self
            .layout
            .get(&self.editor.graph().name, node)
            .map(|(x, y)| Point { x, y })
            .unwrap_or(Point {
                x: 35 + (index % 3) as i32 * 280,
                y: 95 + (index / 3) as i32 * 230,
            });
        let ports = self
            .editor
            .graph()
            .nodes
            .iter()
            .find(|n| n.id == node)
            .map_or(1, |n| n.inputs.len().max(n.outputs.len()).min(64));
        RECT {
            left: p.x,
            top: p.y - self.scroll,
            right: p.x + 230,
            bottom: p.y - self.scroll + 60 + ports as i32 * 20,
        }
    }
    fn focus_node(&mut self, graph: &str, node: NodeId) {
        if let Some(index) = self
            .editor
            .graph_names()
            .iter()
            .position(|name| *name == graph)
        {
            let _ = self.editor.select_graph(index);
        }
        self.selected = Some(node);
        if let Some(index) = self.editor.graph().nodes.iter().position(|n| n.id == node) {
            let rect = self.node_rect(node, index);
            if rect.top < 60 || rect.bottom > self.height - 175 {
                self.scroll = (rect.top + self.scroll - 95).clamp(0, 100_000);
            }
        }
    }
    fn source(&self, source: &SourceEndpoint) -> Option<Point> {
        match source {
            SourceEndpoint::GraphInput(id) => self
                .editor
                .graph()
                .inputs
                .iter()
                .position(|p| p.id == *id)
                .map(|i| Point {
                    x: 25,
                    y: 40 + i as i32 * 22,
                }),
            SourceEndpoint::NodeOutput { node, port } => {
                let i = self
                    .editor
                    .graph()
                    .nodes
                    .iter()
                    .position(|n| n.id == *node)?;
                let j = self.editor.graph().nodes[i]
                    .outputs
                    .iter()
                    .position(|p| p.id == *port)?;
                let r = self.node_rect(*node, i);
                Some(Point {
                    x: r.right,
                    y: r.top + 45 + j as i32 * 20,
                })
            }
        }
    }
    fn target(&self, target: &TargetEndpoint) -> Option<Point> {
        match target {
            TargetEndpoint::GraphOutput(id) => self
                .editor
                .graph()
                .outputs
                .iter()
                .position(|p| p.id == *id)
                .map(|i| Point {
                    x: self.width - 25,
                    y: 40 + i as i32 * 22,
                }),
            TargetEndpoint::NodeInput { node, port } => {
                let i = self
                    .editor
                    .graph()
                    .nodes
                    .iter()
                    .position(|n| n.id == *node)?;
                let j = self.editor.graph().nodes[i]
                    .inputs
                    .iter()
                    .position(|p| p.id == *port)?;
                let r = self.node_rect(*node, i);
                Some(Point {
                    x: r.left,
                    y: r.top + 45 + j as i32 * 20,
                })
            }
        }
    }
}
fn layout_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".layout");
    PathBuf::from(name)
}
fn load_layout(path: &Path) -> EditorLayout {
    let mut bytes = Vec::new();
    let result = fs::File::open(layout_path(path))
        .and_then(|file| file.take(1024 * 1024 + 1).read_to_end(&mut bytes));
    if result.is_ok() {
        EditorLayout::decode(&bytes).unwrap_or_default()
    } else {
        EditorLayout::default()
    }
}
fn save_layout(path: &Path, layout: &EditorLayout) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = layout.encode().map_err(|e| format!("{e:?}"))?;
    save_bytes(&layout_path(path), &bytes)
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
unsafe fn read_edit_text(control: HWND, limit: usize) -> Result<String, String> {
    let length = unsafe { GetWindowTextLengthW(control) };
    if length < 0 || length as usize > limit {
        return Err(
            "El contenido supera el límite del formulario; no se aplicó ningún cambio.".into(),
        );
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let read = unsafe { GetWindowTextW(control, buffer.as_mut_ptr(), buffer.len() as i32) };
    if read != length {
        return Err("El contenido cambió mientras se leía; vuelve a aplicar.".into());
    }
    String::from_utf16(&buffer[..read as usize])
        .map_err(|_| "El texto contiene UTF-16 inválido.".into())
}
fn read_document(path: &Path) -> Result<GraphEditor, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    GraphEditor::decode(&bytes).map_err(|e| format!("{e:?}").into())
}
fn save_document(path: &Path, editor: &GraphEditor) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = editor.encode().map_err(|e| format!("{e:?}"))?;
    save_bytes(path, &bytes)
}
fn save_bytes(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let temp = path.with_file_name(format!(
        ".g0-editor-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result?;
    Ok(())
}

pub fn run(args: &[OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let smoke = args.first().is_some_and(|a| a == "--smoke-test");
    if (smoke && args.len() != 3) || (!smoke && args.len() > 1) {
        return Err("invalid editor arguments".into());
    }
    let path = (!smoke).then(|| args.first().map(PathBuf::from)).flatten();
    let editor = if let Some(path) = &path {
        read_document(path)?
    } else {
        GraphEditor::new()
    };
    unsafe {
        let instance = GetModuleHandleW(null());
        let class = wide("G0NativeGraphEditor");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let owner = Rc::new(RefCell::new(Window::new(editor, path)));
        let state = Box::into_raw(Box::new(owner.clone()));
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("G0 — Editor nativo de grafos").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1120,
            800,
            null_mut(),
            menu(),
            instance,
            state.cast(),
        );
        if hwnd.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        if smoke {
            {
                let mut window = owner.borrow_mut();
                let value = window.editor.add_integer(7).map_err(|e| format!("{e:?}"))?;
                let add = window
                    .editor
                    .add_operation(Operation::Add)
                    .map_err(|e| format!("{e:?}"))?;
                window
                    .editor
                    .connect(
                        SourceEndpoint::NodeOutput { node: 1, port: 0 },
                        TargetEndpoint::NodeInput { node: add, port: 0 },
                    )
                    .map_err(|e| format!("{e:?}"))?;
                window
                    .editor
                    .connect(
                        SourceEndpoint::NodeOutput {
                            node: value,
                            port: 0,
                        },
                        TargetEndpoint::NodeInput { node: add, port: 1 },
                    )
                    .map_err(|e| format!("{e:?}"))?;
                window
                    .editor
                    .connect(
                        SourceEndpoint::NodeOutput { node: add, port: 0 },
                        TargetEndpoint::GraphOutput(0),
                    )
                    .map_err(|e| format!("{e:?}"))?;
                window.selected = Some(add);
                save_document(Path::new(&args[2]), &window.editor)?;
                window.layout.set("main", 1, 40, 90).unwrap();
                window.layout.set("main", value, 40, 300).unwrap();
                window.layout.set("main", add, 370, 190).unwrap();
                save_layout(Path::new(&args[2]), &window.layout)?;
            }
            SendMessageW(hwnd, WM_COMMAND, RUN, 0);
            let window = owner.borrow();
            render_bitmap(&window, Path::new(&args[1]))?;
            drop(window);
            SendMessageW(hwnd, WM_COMMAND, DEBUG_START, 0);
            let control = owner
                .borrow()
                .debugger
                .as_ref()
                .ok_or("debug session not started")?
                .control();
            if control
                .wait_paused(std::time::Duration::from_secs(2))
                .is_none()
            {
                return Err("native debugger failed to pause".into());
            }
            SendMessageW(hwnd, WM_COMMAND, DEBUG_CONTINUE, 0);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while owner.borrow().debugger.is_some() {
                SendMessageW(hwnd, WM_TIMER, 1, 0);
                if std::time::Instant::now() > deadline {
                    return Err("native debugger failed to finish".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            if owner.borrow().trace.len() != 3 {
                return Err("native debugger missing real node events".into());
            }
            SendMessageW(hwnd, WM_COMMAND, SCHEMA_FORM, 0);
            SendMessageW(hwnd, WM_COMMAND, FORM_APPLY, 0);
            SendMessageW(hwnd, WM_COMMAND, NODE_FORM, 0);
            SetWindowTextW(owner.borrow().diagnostic,wide("operation=MakeRecord Point x\r\ninputs=x:int(-100,100)\r\noutputs=value:record(Point)\r\neffects=\r\ncapabilities=").as_ptr());
            SendMessageW(hwnd, WM_COMMAND, FORM_APPLY, 0);
            if !owner
                .borrow()
                .editor
                .graph()
                .nodes
                .iter()
                .any(|n| matches!(n.operation, Operation::MakeRecord { .. }))
            {
                return Err("native typed form failed to create record node".into());
            }
            let text = "ñ".repeat(200);
            let constant = {
                let mut window = owner.borrow_mut();
                let constant = window
                    .editor
                    .add_literal(Literal::Text(text.clone()))
                    .map_err(|e| format!("{e:?}"))?;
                window.selected = Some(constant);
                SetWindowTextW(window.literal, wide(&text).as_ptr());
                constant
            };
            SendMessageW(hwnd, WM_COMMAND, APPLY, 0);
            if owner
                .borrow()
                .editor
                .graph()
                .nodes
                .iter()
                .find(|node| node.id == constant)
                .is_none_or(|node| node.operation != Operation::Const(Literal::Text(text)))
            {
                return Err("native literal Apply silently truncated text".into());
            }
            DestroyWindow(hwnd);
            println!(
                "native editor smoke test: graph saved, executed, rendered, debugged; schema and typed node forms applied"
            );
            return Ok(());
        }
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, null_mut(), 0, 0);
            if result == 0 {
                break;
            }
            if result == -1 {
                return Err(std::io::Error::last_os_error().into());
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

unsafe fn menu() -> HMENU {
    unsafe {
        let menu = CreateMenu();
        for (title, items) in [
            (
                "Archivo",
                vec![
                    (NEW, "Nuevo"),
                    (OPEN, "Abrir..."),
                    (SAVE, "Guardar"),
                    (SAVE_AS, "Guardar como..."),
                ],
            ),
            (
                "Toolbox",
                toolbox()
                    .iter()
                    .enumerate()
                    .map(|(i, (name, _))| (TOOLBOX + i, *name))
                    .collect(),
            ),
            (
                "Propiedades",
                vec![
                    (PROPERTIES, "Ver propiedades del nodo"),
                    (
                        APPLY_TYPE,
                        "Tipo de puerto: in|out ID bool|text|bytes|int MIN MAX",
                    ),
                    (OUTPUT_TYPE, "Resultado desde salida seleccionada"),
                    (ADD_GRAPH, "Crear grafo con nombre del formulario"),
                    (SUBGRAPH, "Llamar grafo con nombre del formulario"),
                    (DUPLICATE, "Duplicar operación seleccionada"),
                    (NODE_FORM, "Crear operación con contrato tipado"),
                    (SCHEMA_FORM, "Crear o actualizar schema"),
                    (SCHEMA_VIEW, "Ver registro de schemas"),
                    (FORM_CANCEL, "Cerrar formulario"),
                ],
            ),
            (
                "Contratos",
                vec![
                    (EDIT_NODE_FORM, "Editar contrato completo del nodo"),
                    (GRAPH_FORM, "Editar interfaz del grafo"),
                ],
            ),
            (
                "Editar",
                vec![
                    (UNDO, "Deshacer"),
                    (REDO, "Rehacer"),
                    (ADD, "Constante 42"),
                    (PLUS, "Suma"),
                    (DELETE, "Eliminar nodo"),
                    (NEXT_GRAPH, "Siguiente grafo"),
                ],
            ),
            (
                "Ejecutar",
                vec![
                    (CHECK, "Validar"),
                    (RUN, "Ejecutar y trazar"),
                    (TRACE, "Siguiente paso de traza"),
                    (TRACE_PREVIOUS, "Paso anterior de traza"),
                    (DEBUG_START, "Depurar: pausa antes del primer nodo"),
                    (DEBUG_STEP, "Paso real"),
                    (DEBUG_CONTINUE, "Continuar"),
                    (DEBUG_PAUSE, "Pausar antes del siguiente nodo"),
                    (DEBUG_STOP, "Detener"),
                    (DEBUG_BREAK, "Alternar breakpoint del nodo"),
                ],
            ),
        ] {
            let popup = CreatePopupMenu();
            for (id, text) in items {
                AppendMenuW(popup, MF_STRING, id, wide(text).as_ptr());
            }
            AppendMenuW(menu, MF_POPUP, popup as usize, wide(title).as_ptr());
        }
        menu
    }
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        dispatch(hwnd, message, wparam, lparam)
    }))
    .unwrap_or_else(|_| unsafe { DefWindowProcW(hwnd, message, wparam, lparam) })
}
unsafe fn dispatch(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if message == WM_NCCREATE && GetWindowLongPtrW(hwnd, GWLP_USERDATA) == 0 {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Rc<RefCell<Window>>;
        if message == WM_NCDESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if !state.is_null() {
                drop(Box::from_raw(state));
            }
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        if state.is_null() {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        if !matches!(
            message,
            WM_CREATE
                | WM_SIZE
                | WM_PAINT
                | WM_COMMAND
                | WM_LBUTTONDOWN
                | WM_MOUSEMOVE
                | WM_LBUTTONUP
                | WM_MOUSEWHEEL
                | WM_DESTROY
                | WM_TIMER
        ) {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        // Modal dialogs and edit controls reenter the window procedure. RefCell
        // keeps those callbacks from producing overlapping mutable Rust references.
        let owner = (*state).clone();
        let Ok(mut window) = owner.try_borrow_mut() else {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        };
        match message {
            WM_CREATE => {
                let instance = GetModuleHandleW(null());
                window.diagnostic = CreateWindowExW(
                    0,
                    wide("EDIT").as_ptr(),
                    wide(&window.status).as_ptr(),
                    WS_CHILD | WS_VISIBLE | ES_MULTILINE as u32 | ES_READONLY as u32 | WS_VSCROLL,
                    10,
                    500,
                    1000,
                    130,
                    hwnd,
                    null_mut(),
                    instance,
                    null(),
                );
                window.literal = CreateWindowExW(
                    0,
                    wide("EDIT").as_ptr(),
                    wide("42").as_ptr(),
                    WS_CHILD | WS_VISIBLE | WS_BORDER | ES_AUTOHSCROLL as u32,
                    10,
                    650,
                    280,
                    26,
                    hwnd,
                    null_mut(),
                    instance,
                    null(),
                );
                SendMessageW(window.literal, 0x00c5, 131072, 0); // EM_LIMITTEXT: 64KiB bytes as hex
                window.apply = CreateWindowExW(
                    0,
                    wide("BUTTON").as_ptr(),
                    wide("Aplicar valor tipado al nodo seleccionado").as_ptr(),
                    WS_CHILD | WS_VISIBLE,
                    300,
                    650,
                    300,
                    26,
                    hwnd,
                    APPLY as HMENU,
                    instance,
                    null(),
                );
                window.operation_choice = CreateWindowExW(
                    0,
                    wide("COMBOBOX").as_ptr(),
                    null(),
                    WS_CHILD | WS_VISIBLE | CBS_DROPDOWNLIST as u32 | WS_VSCROLL,
                    610,
                    650,
                    240,
                    400,
                    hwnd,
                    OPERATION_CHOICE as HMENU,
                    instance,
                    null(),
                );
                for name in OPERATION_NAMES {
                    SendMessageW(
                        window.operation_choice,
                        0x0143,
                        0,
                        wide(name).as_ptr() as LPARAM,
                    );
                }
                SendMessageW(window.operation_choice, 0x014e, 4, 0);
                window.form_apply = CreateWindowExW(
                    0,
                    wide("BUTTON").as_ptr(),
                    wide("Aplicar contrato").as_ptr(),
                    WS_CHILD | WS_VISIBLE,
                    860,
                    650,
                    170,
                    26,
                    hwnd,
                    FORM_APPLY as HMENU,
                    instance,
                    null(),
                );
                SendMessageW(window.diagnostic, 0x00c5, 8192, 0);
                if window.diagnostic.is_null()
                    || window.literal.is_null()
                    || window.apply.is_null()
                    || window.operation_choice.is_null()
                    || window.form_apply.is_null()
                {
                    return -1;
                }
                0
            }
            WM_SIZE => {
                window.width = (lparam as u32 & 0xffff) as i32;
                window.height = ((lparam as u32 >> 16) & 0xffff) as i32;
                let height = window.height;
                let width = window.width;
                MoveWindow(
                    window.diagnostic,
                    10,
                    height - 165,
                    (width - 20).max(1),
                    120,
                    1,
                );
                MoveWindow(window.literal, 10, height - 35, 200, 26, 1);
                MoveWindow(window.apply, 220, height - 35, 280, 26, 1);
                MoveWindow(window.operation_choice, 510, height - 35, 240, 400, 1);
                MoveWindow(window.form_apply, 760, height - 35, 170, 26, 1);
                InvalidateRect(hwnd, null(), 1);
                0
            }
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut paint);
                draw(&mut window, dc);
                EndPaint(hwnd, &paint);
                0
            }
            WM_COMMAND => {
                let id = wparam & 0xffff;
                if id == 0 || id == OPERATION_CHOICE && (wparam >> 16) != CBN_SELCHANGE as usize {
                    return 0;
                }
                command(hwnd, &mut window, id);
                InvalidateRect(hwnd, null(), 1);
                0
            }
            WM_TIMER => {
                let Some(session) = window.debugger.as_ref() else {
                    return 0;
                };
                let control = session.control();
                let completed = session.try_result();
                window.trace = control.trace();
                if let Some((graph, node)) = control.location() {
                    window.focus_node(&graph, node);
                    window.status = format!(
                        "PAUSA REAL antes de {graph} / nodo {node}. Paso, Continuar o Detener.\r\n{} eventos ejecutados; el nodo resaltado todavía no ejecutó.",
                        window.trace.len()
                    );
                }
                if let Some(result) = completed {
                    window.status = match result {
                        Ok(run) => format!(
                            "Depuración completada: {:?}\r\n{} pasos",
                            run.values, run.steps
                        ),
                        Err(error) => format!("Depuración detenida: {error:?}"),
                    };
                    window.debugger = None;
                    KillTimer(hwnd, 1);
                }
                SetWindowTextW(window.diagnostic, wide(&window.status).as_ptr());
                InvalidateRect(hwnd, null(), 1);
                0
            }
            WM_LBUTTONDOWN => {
                if window.schema_form.is_some() {
                    return 0;
                }
                let point = Point {
                    x: lparam as u16 as i16 as i32,
                    y: (lparam as u32 >> 16) as u16 as i16 as i32,
                };
                let hit = window.ports.iter().find(|hit| {
                    (hit.point.x - point.x).abs() <= 9 && (hit.point.y - point.y).abs() <= 9
                });
                if let Some(hit) = hit {
                    if window.debugger.is_some() {
                        window.status = "Detén la depuración antes de cambiar conexiones.".into();
                        SetWindowTextW(window.diagnostic, wide(&window.status).as_ptr());
                        return 0;
                    }
                    match &hit.endpoint {
                        Endpoint::Source(source) => {
                            window.pending = Some(source.clone());
                            window.status =
                                "Salida seleccionada; elige la entrada o el resultado del grafo."
                                    .into();
                        }
                        Endpoint::Target(target) => {
                            let target = target.clone();
                            if let Some(source) = window.pending.take() {
                                window.status = match window.editor.connect(source, target) {
                                    Ok(()) => "Conexión creada.".into(),
                                    Err(e) => format!("Conexión rechazada: {e:?}"),
                                };
                            }
                        }
                    }
                } else {
                    let selected =
                        window
                            .editor
                            .graph()
                            .nodes
                            .iter()
                            .enumerate()
                            .find_map(|(i, n)| {
                                let r = window.node_rect(n.id, i);
                                (point.x >= r.left
                                    && point.x <= r.right
                                    && point.y >= r.top
                                    && point.y <= r.bottom)
                                    .then_some((n.id, r))
                            });
                    if let Some((id, r)) = selected {
                        window.selected = Some(id);
                        window.drag = Some((
                            id,
                            Point {
                                x: point.x - r.left,
                                y: point.y - r.top,
                            },
                        ));
                        if let Some(node) = window.editor.graph().nodes.iter().find(|n| n.id == id)
                        {
                            let value = match &node.operation {
                                Operation::Const(Literal::Integer(v)) => v.to_string(),
                                Operation::Const(Literal::Bool(v)) => v.to_string(),
                                Operation::Const(Literal::Text(v)) => v.clone(),
                                Operation::Const(Literal::Bytes(v)) => {
                                    v.iter().map(|b| format!("{b:02x}")).collect()
                                }
                                _ => String::new(),
                            };
                            SetWindowTextW(window.literal, wide(&value).as_ptr());
                            window.status = window.editor.node_properties(id).unwrap_or_default();
                        }
                    }
                }
                SetWindowTextW(window.diagnostic, wide(&window.status).as_ptr());
                InvalidateRect(hwnd, null(), 1);
                0
            }
            WM_MOUSEMOVE => {
                if wparam & 1 == 0 {
                    window.drag = None;
                } else if let Some((id, offset)) = window.drag {
                    let x = lparam as u16 as i16 as i32;
                    let y = (lparam as u32 >> 16) as u16 as i16 as i32;
                    let scroll = window.scroll;
                    let name = window.editor.graph().name.clone();
                    let _ = window.layout.set(
                        &name,
                        id,
                        (x - offset.x).clamp(0, 100_000),
                        (y - offset.y + scroll).clamp(60, 100_000),
                    );
                    InvalidateRect(hwnd, null(), 1);
                }
                0
            }
            WM_LBUTTONUP => {
                window.drag = None;
                0
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam >> 16) as u16 as i16 as i32).signum() * 60;
                window.scroll = (window.scroll - delta).clamp(0, 100_000);
                InvalidateRect(hwnd, null(), 1);
                0
            }
            WM_DESTROY => {
                if let Some(session) = window.debugger.take() {
                    session.control().stop();
                }
                KillTimer(hwnd, 1);
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

unsafe fn command(hwnd: HWND, window: &mut Window, id: usize) {
    unsafe {
        if window.debugger.is_some()
            && !matches!(
                id,
                DEBUG_STEP
                    | DEBUG_CONTINUE
                    | DEBUG_PAUSE
                    | DEBUG_STOP
                    | DEBUG_BREAK
                    | PROPERTIES
                    | TRACE
                    | TRACE_PREVIOUS
            )
        {
            window.status = "Termina o detén la depuración antes de editar el documento.".into();
            SetWindowTextW(window.diagnostic, wide(&window.status).as_ptr());
            return;
        }
        if window.schema_form.is_some()
            && !matches!(
                id,
                NODE_FORM
                    | OPERATION_CHOICE
                    | SCHEMA_FORM
                    | EDIT_NODE_FORM
                    | GRAPH_FORM
                    | FORM_APPLY
                    | FORM_CANCEL
            )
        {
            window.status = "Aplica o cierra el formulario antes de cambiar el documento.".into();
            SetWindowTextW(hwnd, wide(&window.status).as_ptr());
            return;
        }
        let result: Result<String, String> = (|| match id {
            NODE_FORM | OPERATION_CHOICE => {
                let index = SendMessageW(window.operation_choice, 0x0147, 0, 0).max(0) as usize;
                let name = OPERATION_NAMES.get(index).ok_or("Operación inválida")?;
                window.schema_form = Some(FormMode::Node);
                SendMessageW(window.diagnostic, 0x00cf, 0, 0);
                let template = if *name == "Add" {
                    "operation=Add\r\ninputs=a:int(-1000000,1000000);b:int(-1000000,1000000)\r\noutputs=result:int(-2000000,2000000)\r\neffects=\r\ncapabilities=".into()
                } else {
                    format!("operation={name}\r\ninputs=\r\noutputs=\r\neffects=\r\ncapabilities=")
                };
                SetWindowTextW(window.diagnostic, wide(&template).as_ptr());
                Ok("Completa parámetros y puertos; Aplicar contrato valida antes de crear.".into())
            }
            SCHEMA_FORM => {
                window.schema_form = Some(FormMode::Schema);
                SendMessageW(window.diagnostic, 0x00cf, 0, 0);
                SetWindowTextW(
                    window.diagnostic,
                    wide("name=Point\r\nversion=1\r\nfields=1,x,required,int(-100,100)").as_ptr(),
                );
                Ok("Schema: fields=tag,name,required|optional,type;...".into())
            }
            FORM_APPLY => {
                let kind = window.schema_form.ok_or("Abre un formulario de contrato")?;
                let text = read_edit_text(window.diagnostic, 8192)?;
                match kind {
                    FormMode::Schema => window
                        .editor
                        .apply_schema_form(&text)
                        .map_err(|e| format!("{e:?}"))?,
                    FormMode::Node => {
                        window.selected = Some(
                            window
                                .editor
                                .apply_node_form(&text)
                                .map_err(|e| format!("{e:?}"))?,
                        )
                    }
                    FormMode::EditNode(id) => window
                        .editor
                        .apply_node_form_to(id, &text)
                        .map_err(|e| format!("{e:?}"))?,
                    FormMode::Graph => window
                        .editor
                        .apply_graph_form(&text)
                        .map_err(|e| format!("{e:?}"))?,
                }
                window.schema_form = None;
                window.trace.clear();
                SendMessageW(window.diagnostic, 0x00cf, 1, 0);
                Ok("Contrato insertado. Conecta y valida el programa antes de guardar.".into())
            }
            FORM_CANCEL => {
                window.schema_form = None;
                SendMessageW(window.diagnostic, 0x00cf, 1, 0);
                Ok("Formulario cerrado.".into())
            }
            EDIT_NODE_FORM | GRAPH_FORM => {
                let (kind, text) = if id == GRAPH_FORM {
                    (FormMode::Graph, window.editor.graph_form_text())
                } else {
                    let node = window.selected.ok_or("Selecciona un nodo")?;
                    (
                        FormMode::EditNode(node),
                        window
                            .editor
                            .node_form_text(node)
                            .map_err(|e| format!("{e:?}"))?,
                    )
                };
                window.schema_form = Some(kind);
                SendMessageW(window.diagnostic, 0x00cf, 0, 0);
                SetWindowTextW(window.diagnostic, wide(&text).as_ptr());
                Ok("Edita contrato y elige Aplicar contrato. ID del nodo conservado.".into())
            }
            SCHEMA_VIEW => Ok(window.editor.schema_properties()),
            NEW => {
                window.editor = GraphEditor::new();
                window.path = None;
                window.layout = EditorLayout::default();
                window.trace.clear();
                window.selected = None;
                window.scroll = 0;
                window.breakpoints.clear();
                Ok("Nuevo grafo nativo.".into())
            }
            OPEN => {
                if let Some(path) = file_dialog(hwnd, false) {
                    window.editor = read_document(&path).map_err(|e| e.to_string())?;
                    window.layout = load_layout(&path);
                    window.path = Some(path);
                    window.trace.clear();
                    window.selected = None;
                    window.scroll = 0;
                    window.breakpoints.clear();
                } else {
                    return Ok("Apertura cancelada.".into());
                }
                Ok("Definiciones cargadas sin ejecutarlas ni conceder capacidades.".into())
            }
            SAVE | SAVE_AS => {
                let path = if id == SAVE {
                    window.path.clone().or_else(|| file_dialog(hwnd, true))
                } else {
                    file_dialog(hwnd, true)
                };
                if let Some(path) = path {
                    save_document(&path, &window.editor).map_err(|e| e.to_string())?;
                    save_layout(&path, &window.layout).map_err(|e| e.to_string())?;
                    window.path = Some(path);
                } else {
                    return Ok("Guardado cancelado.".into());
                }
                Ok("Documento nativo validado y guardado.".into())
            }
            UNDO => {
                window.editor.undo().map_err(|e| format!("{e:?}"))?;
                window.trace.clear();
                Ok("Cambio deshecho.".into())
            }
            REDO => {
                window.editor.redo().map_err(|e| format!("{e:?}"))?;
                window.trace.clear();
                Ok("Cambio rehecho.".into())
            }
            ADD | PLUS => {
                let node = if id == ADD {
                    window.editor.add_integer(42)
                } else {
                    window.editor.add_operation(Operation::Add)
                }
                .map_err(|e| format!("{e:?}"))?;
                window.selected = Some(node);
                window.trace.clear();
                Ok(format!(
                    "Nodo {node} creado. Conecta los puertos antes de guardar."
                ))
            }
            DELETE => {
                window
                    .editor
                    .delete_node(window.selected.ok_or("Selecciona un nodo")?)
                    .map_err(|e| format!("{e:?}"))?;
                window.selected = None;
                window.trace.clear();
                Ok("Nodo eliminado.".into())
            }
            NEXT_GRAPH => {
                let names = window.editor.graph_names();
                let current = names
                    .iter()
                    .position(|n| *n == window.editor.graph().name)
                    .unwrap_or(0);
                let index = (current + 1) % names.len();
                window
                    .editor
                    .select_graph(index)
                    .map_err(|e| format!("{e:?}"))?;
                window.selected = None;
                Ok(format!("Grafo: {}", window.editor.graph().name))
            }
            APPLY => {
                let value = read_edit_text(window.literal, 131072)?;
                window
                    .editor
                    .set_literal_text(
                        window.selected.ok_or("Selecciona una constante entera")?,
                        &value,
                    )
                    .map_err(|e| format!("{e:?}"))?;
                window.trace.clear();
                Ok("Constante actualizada. Valida las conexiones afectadas.".into())
            }
            CHECK => {
                window.editor.validate().map_err(|e| format!("{e:?}"))?;
                Ok("Grafo y contratos válidos.".into())
            }
            RUN => {
                let run = window.editor.run().map_err(|e| format!("{e:?}"))?;
                window.trace = run.trace;
                window.trace_index = 0;
                Ok(format!(
                    "Resultado: {:?}\r\nPasos: {}\r\nLa traza conserva IDs de grafo y nodo; los valores sensibles se redactan.",
                    run.values, run.steps
                ))
            }
            TRACE => {
                let event = window
                    .trace
                    .get(window.trace_index)
                    .ok_or("Ejecuta el grafo para obtener una traza; no quedan más eventos")?
                    .clone();
                window.focus_node(&event.graph, event.node);
                window.trace_index += 1;
                Ok(format!(
                    "Traza {}/{} — {} / nodo {}\r\nSalidas: {:?}",
                    window.trace_index,
                    window.trace.len(),
                    event.graph,
                    event.node,
                    event.outputs
                ))
            }
            TRACE_PREVIOUS => {
                window.trace_index = window.trace_index.saturating_sub(2);
                let event = window
                    .trace
                    .get(window.trace_index)
                    .ok_or("No hay traza")?
                    .clone();
                window.focus_node(&event.graph, event.node);
                window.trace_index += 1;
                Ok(format!(
                    "Traza {}/{} — {} / nodo {}\r\nSalidas: {:?}",
                    window.trace_index,
                    window.trace.len(),
                    event.graph,
                    event.node,
                    event.outputs
                ))
            }
            DEBUG_START => {
                let session = window
                    .editor
                    .start_debugger()
                    .map_err(|e| format!("{e:?}"))?;
                for (graph, node) in &window.breakpoints {
                    session
                        .control()
                        .toggle_breakpoint(graph, *node)
                        .map_err(|e| format!("{e:?}"))?;
                }
                window.debugger = Some(session);
                window.trace.clear();
                window.trace_index = 0;
                SetTimer(hwnd, 1, 100, None);
                Ok("Depurador iniciado en un hilo con snapshot del documento.".into())
            }
            DEBUG_STEP | DEBUG_CONTINUE | DEBUG_PAUSE | DEBUG_STOP => {
                let control = window.debugger.as_ref().ok_or("Inicia Depurar")?.control();
                match id {
                    DEBUG_STEP => control.step(),
                    DEBUG_CONTINUE => control.continue_run(),
                    DEBUG_PAUSE => control.pause(),
                    _ => control.stop(),
                }
                Ok("Comando enviado al ejecutor.".into())
            }
            DEBUG_BREAK => {
                let key = (
                    window.editor.graph().name.clone(),
                    window.selected.ok_or("Selecciona un nodo")?,
                );
                let enabled = if window.breakpoints.remove(&key) {
                    false
                } else {
                    if window.breakpoints.len() >= 4096 {
                        return Err("Límite de breakpoints".into());
                    }
                    window.breakpoints.insert(key.clone());
                    true
                };
                if let Some(session) = &window.debugger {
                    session
                        .control()
                        .toggle_breakpoint(&key.0, key.1)
                        .map_err(|e| format!("{e:?}"))?;
                }
                Ok(format!("Breakpoint {} / {}: {}", key.0, key.1, enabled))
            }
            PROPERTIES => window
                .editor
                .node_properties(window.selected.ok_or("Selecciona un nodo")?)
                .map_err(|e| format!("{e:?}")),
            DUPLICATE => {
                let node = window
                    .editor
                    .duplicate_node(window.selected.ok_or("Selecciona un nodo")?)
                    .map_err(|e| format!("{e:?}"))?;
                window.selected = Some(node);
                window.trace.clear();
                window
                    .editor
                    .node_properties(node)
                    .map_err(|e| format!("{e:?}"))
            }
            OUTPUT_TYPE => {
                let source = window
                    .pending
                    .as_ref()
                    .ok_or("Selecciona una salida del nodo")?;
                let ty = match source {
                    SourceEndpoint::NodeOutput { node, port } => window
                        .editor
                        .graph()
                        .nodes
                        .iter()
                        .find(|n| n.id == *node)
                        .and_then(|n| n.outputs.iter().find(|p| p.id == *port))
                        .map(|p| p.ty.clone()),
                    SourceEndpoint::GraphInput(port) => window
                        .editor
                        .graph()
                        .inputs
                        .iter()
                        .find(|p| p.id == *port)
                        .map(|p| p.ty.clone()),
                }
                .ok_or("Puerto no encontrado")?;
                window
                    .editor
                    .set_output_type(0, ty)
                    .map_err(|e| format!("{e:?}"))?;
                Ok("Tipo del resultado actualizado; conecta la salida al resultado.".into())
            }
            ADD_GRAPH | APPLY_TYPE | SUBGRAPH => {
                let text = read_edit_text(window.literal, 128)?;
                if id == ADD_GRAPH {
                    window
                        .editor
                        .add_graph(text.trim())
                        .map_err(|e| format!("{e:?}"))?;
                    return Ok("Grafo creado. Usa Siguiente grafo.".into());
                }
                if id == SUBGRAPH {
                    let node = window
                        .editor
                        .add_subgraph(text.trim())
                        .map_err(|e| format!("{e:?}"))?;
                    window.selected = Some(node);
                    window.trace.clear();
                    return window
                        .editor
                        .node_properties(node)
                        .map_err(|e| format!("{e:?}"));
                }
                let mut fields = text.split_whitespace();
                let output = match fields.next() {
                    Some("in") => false,
                    Some("out") => true,
                    _ => return Err("Formato: in|out ID bool|text|bytes|int MIN MAX".into()),
                };
                let port: PortId = fields
                    .next()
                    .ok_or("Falta ID")?
                    .parse()
                    .map_err(|_| "ID inválido")?;
                let ty = match fields.next() {
                    Some("bool") => SemanticType::Bool,
                    Some("text") => SemanticType::Text,
                    Some("bytes") => SemanticType::Bytes,
                    Some("int") => {
                        let min = fields
                            .next()
                            .ok_or("Falta min")?
                            .parse()
                            .map_err(|_| "Min inválido")?;
                        let max = fields
                            .next()
                            .ok_or("Falta max")?
                            .parse()
                            .map_err(|_| "Max inválido")?;
                        SemanticType::Integer(
                            IntegerType::new(min, max).map_err(|e| e.to_string())?,
                        )
                    }
                    _ => return Err("Tipo inválido".into()),
                };
                if fields.next().is_some() {
                    return Err("Campos adicionales".into());
                }
                window
                    .editor
                    .set_port_type(window.selected, output, port, ty)
                    .map_err(|e| format!("{e:?}"))?;
                Ok("Puerto actualizado; valida antes de ejecutar.".into())
            }
            id if (TOOLBOX..TOOLBOX + toolbox().len()).contains(&id) => {
                let operation = toolbox()[id - TOOLBOX].1.clone();
                let node = match operation {
                    Operation::Const(literal) => window.editor.add_literal(literal),
                    operation => window.editor.add_operation(operation),
                }
                .map_err(|e| format!("{e:?}"))?;
                window.selected = Some(node);
                window.trace.clear();
                window
                    .editor
                    .node_properties(node)
                    .map_err(|e| format!("{e:?}"))
            }
            _ => Ok(window.status.clone()),
        })();
        window.status = result.unwrap_or_else(|e| format!("Error: {e}"));
        if window.schema_form.is_none() {
            SetWindowTextW(window.diagnostic, wide(&window.status).as_ptr());
        } else {
            SetWindowTextW(hwnd, wide(&format!("G0 — {}", window.status)).as_ptr());
        }
    }
}

unsafe fn file_dialog(hwnd: HWND, save: bool) -> Option<PathBuf> {
    unsafe {
        let mut file = vec![0u16; 32768];
        let filter = wide("G0 nativo\0*.g0g;*.g0p\0Todos los archivos\0*.*\0\0");
        let extension = wide("g0g");
        let mut dialog = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: hwnd,
            lpstrFilter: filter.as_ptr(),
            lpstrFile: file.as_mut_ptr(),
            nMaxFile: file.len() as u32,
            lpstrDefExt: extension.as_ptr(),
            Flags: OFN_NOCHANGEDIR
                | OFN_PATHMUSTEXIST
                | if save {
                    OFN_OVERWRITEPROMPT
                } else {
                    OFN_FILEMUSTEXIST
                },
            ..Default::default()
        };
        let ok = if save {
            GetSaveFileNameW(&mut dialog)
        } else {
            GetOpenFileNameW(&mut dialog)
        };
        if ok == 0 {
            return None;
        }
        use std::os::windows::ffi::OsStringExt;
        let end = file.iter().position(|v| *v == 0)?;
        Some(PathBuf::from(OsString::from_wide(&file[..end])))
    }
}

unsafe fn text(dc: HDC, rect: RECT, label: &str) {
    unsafe {
        let label = wide(&label.chars().take(500).collect::<String>());
        let mut rect = rect;
        DrawTextW(
            dc,
            label.as_ptr(),
            -1,
            &mut rect,
            DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
    }
}
unsafe fn draw(window: &mut Window, dc: HDC) {
    unsafe {
        let canvas = RECT {
            left: 0,
            top: 0,
            right: window.width,
            bottom: (window.height - 175).max(1),
        };
        let background = CreateSolidBrush(0x00f5f4f1);
        FillRect(dc, &canvas, background);
        DeleteObject(background);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, 0x002a2a2a);
        let font = GetStockObject(DEFAULT_GUI_FONT);
        let old_font = SelectObject(dc, font);
        text(
            dc,
            RECT {
                left: 15,
                top: 8,
                right: window.width - 15,
                bottom: 28,
            },
            &format!(
                "G0  |  {}  |  {} nodos  |  abrir/guardar definiciones no ejecuta efectos",
                window.editor.graph().name,
                window.editor.graph().nodes.len()
            ),
        );
        let pen = CreatePen(PS_SOLID, 2, 0x008a938f);
        let old_pen = SelectObject(dc, pen);
        for edge in &window.editor.graph().edges {
            if let (Some(from), Some(to)) = (window.source(&edge.from), window.target(&edge.to)) {
                MoveToEx(dc, from.x, from.y, null_mut());
                LineTo(dc, to.x, to.y);
            }
        }
        window.ports.clear();
        for input in &window.editor.graph().inputs {
            let endpoint = SourceEndpoint::GraphInput(input.id);
            if let Some(point) = window.source(&endpoint) {
                Ellipse(dc, point.x - 5, point.y - 5, point.x + 5, point.y + 5);
                text(
                    dc,
                    RECT {
                        left: 35,
                        top: point.y - 8,
                        right: 320,
                        bottom: point.y + 10,
                    },
                    &format!("entrada {}", input.name),
                );
                window.ports.push(Hit {
                    point,
                    endpoint: Endpoint::Source(endpoint),
                });
            }
        }
        for output in &window.editor.graph().outputs {
            let endpoint = TargetEndpoint::GraphOutput(output.id);
            if let Some(point) = window.target(&endpoint) {
                Ellipse(dc, point.x - 5, point.y - 5, point.x + 5, point.y + 5);
                text(
                    dc,
                    RECT {
                        left: window.width - 240,
                        top: point.y - 8,
                        right: window.width - 40,
                        bottom: point.y + 10,
                    },
                    &format!("resultado {}", output.name),
                );
                window.ports.push(Hit {
                    point,
                    endpoint: Endpoint::Target(endpoint),
                });
            }
        }
        for (i, node) in window.editor.graph().nodes.iter().enumerate() {
            let rect = window.node_rect(node.id, i);
            if rect.bottom < 50 || rect.top > canvas.bottom {
                continue;
            }
            let brush = CreateSolidBrush(if window.selected == Some(node.id) {
                0x00f2dfc9
            } else {
                0x00ffffff
            });
            FillRect(dc, &rect, brush);
            DeleteObject(brush);
            FrameRect(dc, &rect, GetStockObject(BLACK_BRUSH));
            text(
                dc,
                RECT {
                    left: rect.left + 10,
                    top: rect.top + 8,
                    right: rect.right - 10,
                    bottom: rect.top + 28,
                },
                &format!(
                    "{}#{} {:?}",
                    if window
                        .breakpoints
                        .contains(&(window.editor.graph().name.clone(), node.id))
                    {
                        "● "
                    } else {
                        ""
                    },
                    node.id,
                    node.operation
                ),
            );
            for port in node.inputs.iter().take(64) {
                let endpoint = TargetEndpoint::NodeInput {
                    node: node.id,
                    port: port.id,
                };
                if let Some(point) = window.target(&endpoint) {
                    Ellipse(dc, point.x - 5, point.y - 5, point.x + 5, point.y + 5);
                    text(
                        dc,
                        RECT {
                            left: point.x + 10,
                            top: point.y - 8,
                            right: rect.left + 110,
                            bottom: point.y + 12,
                        },
                        &format!("{}: {}", port.id, port.name),
                    );
                    window.ports.push(Hit {
                        point,
                        endpoint: Endpoint::Target(endpoint),
                    });
                }
            }
            for port in node.outputs.iter().take(64) {
                let endpoint = SourceEndpoint::NodeOutput {
                    node: node.id,
                    port: port.id,
                };
                if let Some(point) = window.source(&endpoint) {
                    Ellipse(dc, point.x - 5, point.y - 5, point.x + 5, point.y + 5);
                    text(
                        dc,
                        RECT {
                            left: rect.left + 115,
                            top: point.y - 8,
                            right: point.x - 10,
                            bottom: point.y + 12,
                        },
                        &format!("{}: {}", port.name, port.id),
                    );
                    window.ports.push(Hit {
                        point,
                        endpoint: Endpoint::Source(endpoint),
                    });
                }
            }
        }
        SelectObject(dc, old_pen);
        DeleteObject(pen);
        SelectObject(dc, old_font);
    }
}

unsafe fn render_bitmap(window: &Window, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        let width = window.width;
        let height = window.height;
        if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
            return Err("render dimensions outside bounds".into());
        }
        let screen = GetDC(null_mut());
        let dc = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        ReleaseDC(null_mut(), screen);
        if dc.is_null() || bitmap.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let old = SelectObject(dc, bitmap);
        let mut preview = Window::new(
            GraphEditor::decode(&window.editor.encode().map_err(|e| format!("{e:?}"))?)
                .map_err(|e| format!("{e:?}"))?,
            None,
        );
        preview.width = width;
        preview.height = height;
        preview.selected = window.selected;
        preview.layout = window.layout.clone();
        draw(&mut preview, dc);
        let white = CreateSolidBrush(0x00ffffff);
        let lower = RECT {
            left: 0,
            top: height - 175,
            right: width,
            bottom: height,
        };
        FillRect(dc, &lower, white);
        DeleteObject(white);
        for (index, line) in window.status.lines().take(3).enumerate() {
            text(
                dc,
                RECT {
                    left: 15,
                    top: height - 150 + index as i32 * 22,
                    right: width - 15,
                    bottom: height - 128 + index as i32 * 22,
                },
                line,
            );
        }
        text(
            dc,
            RECT {
                left: 15,
                top: height - 80,
                right: width - 15,
                bottom: height - 50,
            },
            "Editor Win32/GDI: puertos tipados, validación, undo/redo y traza nativa. 42 + 7 = 49",
        );
        SelectObject(dc, old);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: 40,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
        let result = GetDIBits(
            dc,
            bitmap,
            0,
            height as u32,
            pixels.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );
        DeleteObject(bitmap);
        DeleteDC(dc);
        if result == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut bytes = b"BM".to_vec();
        bytes.extend_from_slice(&(54u32 + pixels.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&54u32.to_le_bytes());
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&(-height).to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&32u16.to_le_bytes());
        bytes.extend_from_slice(&[0; 24]);
        bytes.extend_from_slice(&pixels);
        fs::write(path, bytes)?;
        Ok(())
    }
}

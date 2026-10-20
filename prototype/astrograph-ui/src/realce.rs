//! Realce e indent por tree-sitter feitos no Astrograph, fora do processo do
//! Neovim (ROADMAP, S1 e S2). Ligados por padrao no app; `ASTROGRAPH_REALCE=nvim`
//! volta ao realce nativo e `ASTROGRAPH_INDENT=nvim` ao indent nativo.
//!
//! O nvim avisa por `rpcnotify` quando um buffer abre ou muda de tipo; aqui o
//! buffer e assinado com `nvim_buf_attach`, e cada `nvim_buf_lines_event`
//! atualiza a copia (`astrograph_syntax::Document`), reparseia so o que mudou e
//! devolve os highlights das linhas afetadas numa chamada atomica. A
//! `indentexpr` do buffer vira um `rpcrequest` respondido daqui (`indent`).
//!
//! O handler do RPC so repassa os eventos para ca: ele roda dentro do loop de
//! leitura do nvim-rs, e uma chamada RPC ali dentro travaria o loop. Nenhum
//! lock do `Estado` fica preso durante uma chamada ao nvim.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use astrograph_syntax::{detect, lang, Document};
use nvim_rs::compat::tokio::Compat;
use nvim_rs::{Neovim, Value};
use tokio::process::ChildStdin;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::Notify;

type Nvim = Neovim<Compat<ChildStdin>>;

/// Extmarks por chamada atomica: o lote e 26x mais rapido que chamadas avulsas,
/// mas um lote gigante seguraria o nvim de uma vez so.
const BATCH: usize = 4000;

/// Quanto o indent espera a copia alcancar a versao do buffer antes de desistir
/// (devolve -1: o nvim mantem o autoindent).
const INDENT_WAIT: Duration = Duration::from_millis(50);

pub enum Event {
    /// buffer aberto ou com tipo novo (autocmd do nvim)
    Buf(i64),
    /// `nvim_buf_lines_event`: buffer, versao, primeira linha, ultima (-1 = fim), linhas, ha mais
    Lines { buf: i64, tick: i64, first: i64, last: i64, lines: Vec<String>, more: bool },
    /// `nvim_buf_changedtick_event`: versao nova sem mudanca de texto
    Tick { buf: i64, tick: i64 },
    Detach(i64),
}

/// Copias dos buffers, compartilhadas entre a task do realce e os pedidos de
/// indent que o nvim faz no meio da digitacao.
#[derive(Default)]
pub struct Estado {
    bufs: Mutex<HashMap<i64, Buf>>,
    /// avisado a cada lote de eventos aplicado (pedidos de indent esperando)
    aplicado: Notify,
    /// avisado quando ha linhas para repintar (task de pintura)
    pintar: Notify,
}

pub type Shared = Arc<Estado>;

/// Quanto cada lote levou do evento chegar ate os extmarks aplicados (so com
/// `ASTROGRAPH_REALCE_TEMPO=1`; para as medicoes do `realce_e2e`).
static TEMPOS: Mutex<Vec<Duration>> = Mutex::new(Vec::new());

pub fn tempos() -> Vec<Duration> {
    TEMPOS.lock().unwrap().clone()
}

/// Lotes de realce que o nvim recusou (para as medicoes e o diagnostico).
pub static ERROS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Pedidos de indent: (espera pela versao do buffer, calculo).
static TEMPOS_INDENT: Mutex<Vec<(Duration, Duration)>> = Mutex::new(Vec::new());

pub fn tempos_indent() -> Vec<(Duration, Duration)> {
    TEMPOS_INDENT.lock().unwrap().clone()
}

pub fn enabled() -> bool {
    std::env::var("ASTROGRAPH_REALCE").map_or(true, |v| v != "nvim")
}

/// Indent em Rust: ligado por padrao (`ASTROGRAPH_INDENT=nvim` volta ao
/// nvim-treesitter). Mesmo resultado em 100% das linhas medidas; depois de uma
/// edicao no meio do arquivo custa 0,2-3,6 ms contra 4,5-9,4 ms do nativo, que
/// varre a arvore inteira (o Rust so o bloco de primeiro nivel da linha).
pub fn indent_enabled() -> bool {
    std::env::var("ASTROGRAPH_INDENT").map_or(true, |v| v != "nvim")
}

/// Traduz as notificacoes do nvim que interessam ao realce.
pub fn parse_notify(name: &str, args: &[Value]) -> Option<Event> {
    match name {
        "astrograph_buf" => Some(Event::Buf(args.first()?.as_i64()?)),
        "nvim_buf_lines_event" => {
            let buf = handle(args.first()?)?;
            let tick = args.get(1).and_then(Value::as_i64).unwrap_or(0);
            let first = args.get(2)?.as_i64()?;
            let last = args.get(3)?.as_i64()?;
            let lines = args
                .get(4)?
                .as_array()?
                .iter()
                .map(|v| v.as_str().unwrap_or_default().to_string())
                .collect();
            let more = args.get(5).and_then(Value::as_bool).unwrap_or(false);
            Some(Event::Lines { buf, tick, first, last, lines, more })
        }
        "nvim_buf_changedtick_event" => {
            Some(Event::Tick { buf: handle(args.first()?)?, tick: args.get(1)?.as_i64()? })
        }
        "nvim_buf_detach_event" => Some(Event::Detach(handle(args.first()?)?)),
        _ => None,
    }
}

/// Numero do buffer: vem como inteiro ou como o tipo `Buffer` (ext do msgpack,
/// com o inteiro codificado dentro).
fn handle(v: &Value) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    let Value::Ext(_, data) = v else { return None };
    let (&tag, rest) = data.split_first()?;
    Some(match tag {
        0x00..=0x7f => tag as i64,
        0xcc => *rest.first()? as i64,
        0xcd => u16::from_be_bytes(rest.get(..2)?.try_into().ok()?) as i64,
        0xce => u32::from_be_bytes(rest.get(..4)?.try_into().ok()?) as i64,
        0xcf => u64::from_be_bytes(rest.get(..8)?.try_into().ok()?) as i64,
        _ => return None,
    })
}

struct Buf {
    doc: Option<Document>,
    lang: &'static astrograph_syntax::Lang,
    /// conteudo inicial chegando em partes (`more = true`)
    pending: Vec<String>,
    /// linhas na tela quando o buffer foi assinado: pintadas primeiro
    view: Range<usize>,
    /// `b:changedtick` ja aplicado na copia
    tick: i64,
    /// a pintar: a parte na tela do arquivo recem-aberto (vai primeiro) e as
    /// linhas mudadas, sempre em coordenadas atuais (deslocadas a cada edicao)
    first_paint: Option<Range<usize>>,
    dirty: Vec<Range<usize>>,
}

/// Desloca as faixas pendentes pela edicao que trocou `[first, last)` por
/// `new_len` linhas: as de depois andam junto; as que tocam a edicao a cobrem.
fn shift_ranges(ranges: &mut [Range<usize>], first: usize, last: usize, new_len: usize) {
    let delta = new_len as isize - (last - first) as isize;
    let mv = |x: usize| (x as isize + delta).max(first as isize) as usize;
    for r in ranges.iter_mut() {
        if r.start >= last {
            *r = mv(r.start)..mv(r.end);
        } else if r.end > first {
            let end = if r.end > last { mv(r.end) } else { first + new_len };
            *r = r.start.min(first)..end.max(first + new_len).max(r.start.min(first));
        }
    }
}

/// Pedido `astrograph_indent` da `indentexpr`: (buffer, linha, changedtick,
/// shiftwidth, tabstop) -> colunas, ou -1 (o nvim mantem o autoindent).
pub async fn indent(estado: &Shared, args: &[Value]) -> Value {
    let num = |i: usize| args.get(i).and_then(Value::as_i64).unwrap_or(-1);
    let (buf, lnum, tick, sw, ts) = (num(0), num(1), num(2), num(3), num(4));
    let t0 = Instant::now();
    let deadline = t0 + INDENT_WAIT;
    loop {
        // registra a espera antes de olhar, para nao perder o aviso
        let notified = estado.aplicado.notified();
        {
            let bufs = estado.bufs.lock().unwrap();
            match bufs.get(&buf) {
                Some(Buf { doc: Some(doc), tick: t, .. }) if *t >= tick => {
                    let sw = if sw > 0 { sw } else { ts.max(1) };
                    let t1 = Instant::now();
                    let r = doc.indent(lnum.max(0) as usize, sw, ts.max(1) as usize);
                    if std::env::var_os("ASTROGRAPH_REALCE_TEMPO").is_some() {
                        TEMPOS_INDENT.lock().unwrap().push((t1 - t0, t1.elapsed()));
                    }
                    return Value::from(r);
                }
                None => return Value::from(-1),
                _ => {}
            }
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || tokio::time::timeout(left, notified).await.is_err() {
            return Value::from(-1);
        }
    }
}

pub async fn run(nvim: Nvim, estado: Shared, mut rx: UnboundedReceiver<Event>) {
    let chan = match nvim.get_api_info().await {
        Ok(info) => info.first().and_then(Value::as_i64).unwrap_or(0),
        Err(_) => return,
    };
    let ns = match nvim.create_namespace("astrograph_realce").await {
        Ok(ns) => ns,
        Err(_) => return,
    };
    let notify = format!("call rpcnotify({chan}, 'astrograph_buf', str2nr(expand('<abuf>')))");
    let _ = nvim
        .create_autocmd(
            Value::from(vec![Value::from("BufReadPost"), Value::from("BufNewFile"), Value::from("FileType")]),
            vec![
                (Value::from("command"), Value::from(notify)),
                (Value::from("desc"), Value::from("Astrograph: realce e indent em Rust")),
            ],
        )
        .await;
    // buffers que ja estavam abertos antes daqui
    if let Ok(list) = nvim.list_bufs().await {
        for b in list {
            if let Some(n) = handle(b.get_value()) {
                open_buf(&nvim, &estado, chan, n).await;
            }
        }
    }

    tokio::spawn(paint_loop(nvim.clone(), estado.clone(), ns));
    // Aplica as edicoes assim que chegam (sem falar com o nvim), para os
    // pedidos de indent nao esperarem a pintura; a pintura e outra task.
    while let Some(first) = rx.recv().await {
        let mut events = vec![first];
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        let mut abrir = Vec::new();
        {
            let mut bufs = estado.bufs.lock().unwrap();
            for ev in events {
                match ev {
                    Event::Buf(n) => abrir.push(n),
                    Event::Tick { buf, tick } => {
                        if let Some(b) = bufs.get_mut(&buf) {
                            b.tick = b.tick.max(tick);
                        }
                    }
                    Event::Lines { buf, tick, first, last, lines, more } => {
                        let Some(b) = bufs.get_mut(&buf) else { continue };
                        match &mut b.doc {
                            None => {
                                b.pending.extend(lines);
                                if !more {
                                    let all = std::mem::take(&mut b.pending);
                                    let doc = Document::new(b.lang, &all);
                                    let n = doc.line_count();
                                    // a parte na tela vai num lote proprio, antes
                                    // do resto (o nativo tambem so pinta a tela no
                                    // primeiro redraw)
                                    let view = b.view.start.min(n)..b.view.end.min(n);
                                    b.dirty = vec![0..view.start, view.end..n];
                                    b.first_paint = Some(view);
                                    b.doc = Some(doc);
                                    b.tick = tick;
                                }
                            }
                            Some(doc) => {
                                let n = doc.line_count();
                                let f = (first.max(0) as usize).min(n);
                                let l = if last < 0 { n } else { (last as usize).clamp(f, n) };
                                shift_ranges(&mut b.dirty, f, l, lines.len());
                                if let Some(v) = b.first_paint.as_mut() {
                                    shift_ranges(std::slice::from_mut(v), f, l, lines.len());
                                }
                                let changed = doc.apply_lines(f, Some(l), &lines);
                                b.dirty.extend(changed);
                                b.tick = b.tick.max(tick);
                            }
                        }
                    }
                    Event::Detach(n) => {
                        bufs.remove(&n);
                    }
                }
            }
        }
        // os pedidos de indent esperando esta versao ja podem responder
        estado.aplicado.notify_waiters();
        estado.pintar.notify_one();
        for n in abrir {
            open_buf(&nvim, &estado, chan, n).await;
        }
    }
}

/// Pinta o que estiver pendente nos buffers: a parte na tela dos recem-abertos
/// primeiro, depois as linhas mudadas. Junta tudo o que chegou enquanto pintava.
async fn paint_loop(nvim: Nvim, estado: Shared, ns: i64) {
    let medir = std::env::var("ASTROGRAPH_REALCE_TEMPO").is_ok();
    loop {
        estado.pintar.notified().await;
        let t0 = Instant::now();
        let (first, rest) = {
            let mut bufs = estado.bufs.lock().unwrap();
            let mut first = Vec::new();
            let mut rest = Vec::new();
            for (&n, b) in bufs.iter_mut() {
                if b.doc.is_none() {
                    continue;
                }
                if let Some(v) = b.first_paint.take() {
                    first.push((n, vec![v]));
                }
                let mut d = std::mem::take(&mut b.dirty);
                d.retain(|r| !r.is_empty());
                if !d.is_empty() {
                    rest.push((n, d));
                }
            }
            (first, rest)
        };
        if first.is_empty() && rest.is_empty() {
            continue;
        }
        for (buf, ranges) in first.into_iter().chain(rest) {
            let calls = paint_calls(&estado, ns, buf, &ranges);
            send(&nvim, calls).await;
        }
        if medir {
            TEMPOS.lock().unwrap().push(t0.elapsed());
        }
    }
}

/// Limites de arquivo grande do astrocore (`features.large_buf`): acima disso o
/// realce nativo tambem fica desligado.
const LARGE_BYTES: i64 = 256 * 1024;
const LARGE_LINES: i64 = 10_000;

/// Buffer aberto ou com tipo novo: se ja assinado, so garante o nativo
/// desligado; senao, decide se ha gramatica e assina.
async fn open_buf(nvim: &Nvim, estado: &Shared, chan: i64, n: i64) {
    // FileType de novo (ex.: `:set ft`): o indent/<lang>.vim do runtime pode ter
    // trocado a indentexpr; reaplica com a linguagem ja escolhida
    let known = estado.bufs.lock().unwrap().get(&n).map(|b| b.lang);
    if let Some(l) = known {
        let _ = nvim.exec_lua(STOP_NATIVE, native_args(chan, n, l).as_array().cloned().unwrap_or_default()).await;
        return;
    }
    if let Some(b) = attach_buf(nvim, chan, n).await {
        estado.bufs.lock().unwrap().insert(n, b);
    }
}

/// Decide se o buffer tem gramatica; se tiver, desliga o realce nativo e assina.
/// Duas idas ao nvim (dados do buffer; desligar + assinar num lote): com a
/// config carregada o loop do nvim esta ocupado e cada ida espera por ele.
async fn attach_buf(nvim: &Nvim, chan: i64, n: i64) -> Option<Buf> {
    let info = nvim
        .exec_lua(
            "local b = ... ; if not vim.api.nvim_buf_is_valid(b) then return nil end; \
             local n = vim.api.nvim_buf_line_count(b); \
             local w, top, bot = vim.fn.bufwinid(b), 0, 80; \
             if w ~= -1 then top = vim.fn.line('w0', w) - 1; bot = vim.fn.line('w$', w) end; \
             return { vim.api.nvim_buf_get_name(b), vim.api.nvim_buf_get_lines(b, 0, 1, false)[1] or '', \
                      vim.bo[b].filetype, n, vim.api.nvim_buf_get_offset(b, n), top, bot }",
            vec![Value::from(n)],
        )
        .await
        .ok()?;
    let info = info.as_array()?;
    let text = |i: usize| info.get(i).and_then(Value::as_str).unwrap_or_default();
    let num = |i: usize| info.get(i).and_then(Value::as_i64).unwrap_or(0);
    if num(3) > LARGE_LINES || num(4) > LARGE_BYTES {
        return None;
    }
    let l = detect(text(0), text(1)).and_then(lang).or_else(|| lang(text(2)))?;
    // o attach so vale por RPC (do Lua/Vimscript os eventos nao viriam para
    // este canal); dentro do lote atomico continua sendo deste canal
    let batch = vec![
        call("nvim_exec_lua", vec![Value::from(STOP_NATIVE), native_args(chan, n, l)]),
        call("nvim_buf_attach", vec![Value::from(n), Value::from(true), Value::Map(vec![])]),
    ];
    let attached = match nvim.call_atomic(batch).await {
        Ok(res) => res
            .first()
            .and_then(Value::as_array)
            .and_then(|r| r.get(1))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        Err(_) => false,
    };
    if !attached {
        eprintln!("ASTROGRAPH_REALCE: nvim_buf_attach({n}) falhou");
        return None;
    }
    let view = num(5).max(0) as usize..num(6).max(0) as usize;
    Some(Buf { doc: None, lang: l, pending: Vec::new(), view, tick: 0, first_paint: None, dirty: Vec::new() })
}

/// Desliga o realce nativo (treesitter do nvim e `syntax`, que fariam o mesmo
/// trabalho em dobro) e, se ha consulta de indent, troca a `indentexpr` por um
/// pedido a este canal. `b:astrograph_realce` avisa a config: o astrocore nao
/// religa o treesitter nem a indentexpr dele neste buffer.
const STOP_NATIVE: &str = "local b, chan, indent = ... ; if not vim.api.nvim_buf_is_valid(b) then return end; \
     vim.b[b].astrograph_realce = true; pcall(vim.treesitter.stop, b); vim.bo[b].syntax = ''; \
     if indent then vim.b[b].astrograph_indent = true; vim.bo[b].indentexpr = \
       ('rpcrequest(%d, \"astrograph_indent\", bufnr(), v:lnum, b:changedtick, shiftwidth(), &tabstop)'):format(chan) end";

fn native_args(chan: i64, n: i64, l: &astrograph_syntax::Lang) -> Value {
    let indent = l.indents.is_some() && indent_enabled();
    Value::from(vec![Value::from(n), Value::from(chan), Value::from(indent)])
}

/// Lote de extmarks das linhas `ranges`, montado com o lock e enviado sem ele.
fn paint_calls(estado: &Shared, ns: i64, buf: i64, ranges: &[Range<usize>]) -> Vec<Value> {
    let bufs = estado.bufs.lock().unwrap();
    let Some(Buf { doc: Some(doc), .. }) = bufs.get(&buf) else { return Vec::new() };
    let mut calls: Vec<Value> = Vec::new();
    for r in ranges {
        calls.push(call(
            "nvim_buf_clear_namespace",
            vec![Value::from(buf), Value::from(ns), Value::from(r.start as i64), Value::from(r.end as i64)],
        ));
        for s in doc.highlights(r.clone()) {
            calls.push(call(
                "nvim_buf_set_extmark",
                vec![
                    Value::from(buf),
                    Value::from(ns),
                    Value::from(s.row as i64),
                    Value::from(s.start_col as i64),
                    Value::Map(vec![
                        (Value::from("end_col"), Value::from(s.end_col as i64)),
                        (Value::from("hl_group"), Value::from(s.group)),
                        (Value::from("priority"), Value::from(100)),
                    ]),
                ],
            ));
        }
    }
    calls
}

async fn send(nvim: &Nvim, mut calls: Vec<Value>) {
    while !calls.is_empty() {
        let rest = calls.split_off(calls.len().min(BATCH));
        // o lote para no primeiro erro (ex.: buffer fechado no meio): o resto
        // nao e aplicado; fica registrado para nao sumir em silencio
        match nvim.call_atomic(calls).await {
            Ok(res) => {
                if let Some(err) = res.get(1).filter(|e| !e.is_nil()) {
                    eprintln!("ASTROGRAPH_REALCE: lote de realce parou: {err}");
                    ERROS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
            Err(e) => {
                eprintln!("ASTROGRAPH_REALCE: lote de realce falhou: {e}");
                ERROS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        calls = rest;
    }
}

fn call(name: &str, args: Vec<Value>) -> Value {
    Value::Array(vec![Value::from(name), Value::Array(args)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faixas_pendentes_andam_com_a_edicao() {
        // Enter na linha 5: troca [5, 6) por 2 linhas; a faixa da linha 10 vai para 11
        let mut r = vec![10..11, 2..3, 5..6];
        shift_ranges(&mut r, 5, 6, 2);
        assert_eq!(r, vec![11..12, 2..3, 5..7]);
        // apagar as linhas [3, 8): o que estava em 10 vai para 5
        let mut r = vec![10..12, 0..1, 4..6];
        shift_ranges(&mut r, 3, 8, 0);
        assert_eq!(r, vec![5..7, 0..1, 3..3]);
    }

    #[test]
    fn numero_do_buffer_em_ext() {
        assert_eq!(handle(&Value::Ext(0, vec![0x05])), Some(5));
        assert_eq!(handle(&Value::Ext(0, vec![0xcd, 0x01, 0x00])), Some(256));
        assert_eq!(handle(&Value::from(7)), Some(7));
    }

    #[test]
    fn evento_de_linhas() {
        let args = vec![
            Value::Ext(0, vec![0x02]),
            Value::from(10),
            Value::from(3),
            Value::from(-1),
            Value::from(vec![Value::from("a"), Value::from("b")]),
            Value::from(false),
        ];
        match parse_notify("nvim_buf_lines_event", &args) {
            Some(Event::Lines { buf: 2, tick: 10, first: 3, last: -1, lines, more: false }) => {
                assert_eq!(lines, ["a", "b"])
            }
            _ => panic!("evento nao reconhecido"),
        }
    }
}

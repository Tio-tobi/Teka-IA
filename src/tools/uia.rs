//! Achar um controle **pelo nome** e acioná-lo, sem enxergar a tela.
//!
//! ## Por que isto existe
//!
//! `Ctrl+Shift+M` alterna o mudo do Discord às cegas: não sabe se ficou mudo ou se
//! desmutou, e depende de o atalho global estar configurado. O UIAutomation — a
//! mesma API que leitor de tela usa — **lê o estado, age, e confirma que mudou**.
//!
//! ## O que foi medido antes de escrever isto (2026-09-06)
//!
//! Nem todo aplicativo publica sua árvore de acessibilidade, e a diferença é
//! enorme:
//!
//! ```text
//! WhatsApp (WebView2)   1238 controles   arvore completa
//! Discord  (Electron)    898 controles   arvore completa
//! XBOX     (UWP)          12
//! Spotify  (Electron)     15, so 2 com nome   -> inutil
//! Radmin   (Win32)         0
//! ```
//!
//! **Não é a tecnologia, é o aplicativo.** Discord e Spotify são os dois Electron e
//! estão nos extremos opostos. Por isso o Spotify continua sendo controlado por
//! tecla de mídia, e só o Discord passa por aqui.
//!
//! ## Três armadilhas que custaram tentativa
//!
//! 1. **Errar índice de vtable não dá erro de compilação.** Chama outra função e o
//!    processo morre ou devolve lixo. Cada índice aqui foi validado sozinho antes
//!    de entrar: `GetRootElement`[5] + `get_CurrentName`[23] devolveram "Área de
//!    Trabalho 1", que é o nome real do elemento raiz.
//! 2. **`GetCurrentPattern` devolve SUCESSO com ponteiro nulo** quando o controle
//!    não suporta o padrão. Testar só o `HRESULT` passa batido.
//! 3. **O botão de mudo não é `Invoke`, é `Toggle`.** Sondando cinco padrões:
//!    `Invoke` nulo, `Toggle` disponível, `LegacyIAccessible` disponível. E o
//!    `Toggle` ainda vem com leitura de estado de brinde.

#![allow(non_snake_case)]

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::sync::Mutex;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid {
        d1: u32,
        d2: u16,
        d3: u16,
        d4: [u8; 8],
    }
    const CLSID_CUIAUTOMATION: Guid = Guid {
        d1: 0xFF48DBA4, d2: 0x60EF, d3: 0x4201,
        d4: [0xAA, 0x87, 0x54, 0x10, 0x3E, 0xEF, 0x59, 0x4E],
    };
    const IID_IUIAUTOMATION: Guid = Guid {
        d1: 0x30CBE57D, d2: 0xD9D0, d3: 0x452A,
        d4: [0xAB, 0x13, 0x7A, 0xC5, 0xAC, 0x48, 0x25, 0xEE],
    };
    const UIA_NAME_PROP: i32 = 30005;
    const UIA_TOGGLE_PATTERN: i32 = 10015;
    const ESCOPO_DESCENDENTES: i32 = 4;

    #[repr(C)]
    struct Variant {
        vt: u16,
        r1: u16,
        r2: u16,
        r3: u16,
        val: [u64; 2],
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(r: *mut c_void, m: u32) -> i32;
        fn CoCreateInstance(
            c: *const Guid, o: *mut c_void, x: u32, i: *const Guid, s: *mut *mut c_void,
        ) -> i32;
    }
    #[link(name = "oleaut32")]
    unsafe extern "system" {
        fn SysAllocString(s: *const u16) -> *mut u16;
        fn SysFreeString(s: *mut u16);
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(f: extern "system" fn(isize, isize) -> i32, p: isize) -> i32;
        fn GetWindowTextW(h: isize, b: *mut u16, n: i32) -> i32;
        fn IsWindowVisible(h: isize) -> i32;
    }

    static ALVO: Mutex<isize> = Mutex::new(0);
    static PROCURA: Mutex<String> = Mutex::new(String::new());

    extern "system" fn visitar(h: isize, _: isize) -> i32 {
        let mut b = [0u16; 256];
        let n = unsafe { GetWindowTextW(h, b.as_mut_ptr(), 256) };
        if n > 0 && unsafe { IsWindowVisible(h) } != 0 {
            let t = String::from_utf16_lossy(&b[..n as usize]);
            if t.contains(PROCURA.lock().unwrap().as_str()) {
                *ALVO.lock().unwrap() = h;
                return 0; // achou, para a enumeracao
            }
        }
        1
    }

    /// Primeira janela VISÍVEL cujo título contenha `trecho`.
    ///
    /// Visível de propósito: um app na bandeja não tem janela, e agir sobre ele por
    /// aqui é impossível — melhor falhar com mensagem clara que achar um handle
    /// morto.
    fn janela_com(trecho: &str) -> Option<isize> {
        *PROCURA.lock().unwrap() = trecho.to_string();
        *ALVO.lock().unwrap() = 0;
        unsafe { EnumWindows(visitar, 0) };
        let h = *ALVO.lock().unwrap();
        (h != 0).then_some(h)
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Ponteiro para o método `i` da vtable.
    ///
    /// # Safety
    /// `o` tem de ser um ponteiro COM vivo, e `i` um índice válido da interface
    /// dele. Errar o índice é comportamento indefinido — ver a nota do módulo.
    unsafe fn vt(o: *mut c_void, i: usize) -> *const c_void {
        unsafe { *(*(o as *const *const *const c_void)).add(i) }
    }

    unsafe fn soltar(o: *mut c_void) {
        if !o.is_null() {
            let r: extern "system" fn(*mut c_void) -> u32 = unsafe { std::mem::transmute(vt(o, 2)) };
            r(o);
        }
    }

    /// O que aconteceu com o controle.
    pub struct Resultado {
        pub antes: i32,
        pub depois: i32,
    }

    /// Acha o controle `controle` dentro da janela cujo título contém `janela`, e
    /// alterna. Devolve o estado antes e depois — é o que separa isto de apertar
    /// uma tecla às cegas.
    pub fn alternar(janela: &str, controle: &str) -> Result<Resultado, String> {
        let Some(hwnd) = janela_com(janela) else {
            return Err(format!(
                "nao achei janela visivel com {janela:?} no titulo (o app esta na bandeja?)"
            ));
        };
        unsafe {
            // S_FALSE (0x1) quando ja inicializado: nao e erro.
            let hr = CoInitializeEx(std::ptr::null_mut(), 2);
            if hr < 0 {
                return Err(format!("CoInitializeEx falhou (0x{hr:08X})"));
            }
            let mut aut: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_CUIAUTOMATION, std::ptr::null_mut(), 1, &IID_IUIAUTOMATION, &mut aut,
            );
            if hr != 0 || aut.is_null() {
                return Err(format!("UIAutomation indisponivel (0x{hr:08X})"));
            }

            // [6] ElementFromHandle
            let f: extern "system" fn(*mut c_void, isize, *mut *mut c_void) -> i32 =
                std::mem::transmute(vt(aut, 6));
            let mut jan: *mut c_void = std::ptr::null_mut();
            if f(aut, hwnd, &mut jan) != 0 || jan.is_null() {
                soltar(aut);
                return Err("nao consegui ler a janela".into());
            }

            let r = achar_e_alternar(aut, jan, controle);
            soltar(jan);
            soltar(aut);
            r
        }
    }

    unsafe fn achar_e_alternar(
        aut: *mut c_void, jan: *mut c_void, controle: &str,
    ) -> Result<Resultado, String> {
        let w = wide(controle);
        let b = unsafe { SysAllocString(w.as_ptr()) };
        let var = Variant { vt: 8, r1: 0, r2: 0, r3: 0, val: [b as u64, 0] }; // VT_BSTR

        // [23] CreatePropertyCondition — consome a VARIANT.
        let cpc: extern "system" fn(*mut c_void, i32, Variant, *mut *mut c_void) -> i32 =
            unsafe { std::mem::transmute(vt(aut, 23)) };
        let mut cond: *mut c_void = std::ptr::null_mut();
        let hr = cpc(aut, UIA_NAME_PROP, var, &mut cond);
        unsafe { SysFreeString(b) };
        if hr != 0 || cond.is_null() {
            return Err(format!("nao montei a condicao de busca (0x{hr:08X})"));
        }

        // [5] FindFirst
        let ff: extern "system" fn(*mut c_void, i32, *mut c_void, *mut *mut c_void) -> i32 =
            unsafe { std::mem::transmute(vt(jan, 5)) };
        let mut el: *mut c_void = std::ptr::null_mut();
        let hr = ff(jan, ESCOPO_DESCENDENTES, cond, &mut el);
        unsafe { soltar(cond) };
        if hr != 0 || el.is_null() {
            return Err(format!("nao achei o controle {controle:?} na janela"));
        }

        let r = unsafe { alternar_elemento(el) };
        unsafe { soltar(el) };
        r
    }

    unsafe fn alternar_elemento(el: *mut c_void) -> Result<Resultado, String> {
        // [16] GetCurrentPattern — CUIDADO: devolve S_OK com ponteiro NULO quando o
        // controle nao suporta o padrao. Testar so o HRESULT passa batido.
        let gp: extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> i32 =
            unsafe { std::mem::transmute(vt(el, 16)) };
        let mut pat: *mut c_void = std::ptr::null_mut();
        if gp(el, UIA_TOGGLE_PATTERN, &mut pat) != 0 || pat.is_null() {
            return Err("esse controle nao e de alternancia (sem padrao Toggle)".into());
        }
        // [4] get_CurrentToggleState  |  [3] Toggle
        let estado: extern "system" fn(*mut c_void, *mut i32) -> i32 =
            unsafe { std::mem::transmute(vt(pat, 4)) };
        let acionar: extern "system" fn(*mut c_void) -> i32 =
            unsafe { std::mem::transmute(vt(pat, 3)) };

        let mut antes = -1;
        estado(pat, &mut antes);
        let hr = acionar(pat);
        if hr != 0 {
            unsafe { soltar(pat) };
            return Err(format!("Toggle falhou (0x{hr:08X})"));
        }

        // ESPERA ATE MUDAR, e nao um tempo fixo.
        //
        // A primeira versao dormia 400 ms e lia uma vez. Deu isto, medido:
        //
        //     1: mutar_discord: desligado -> desligado   (diz que nao mudou)
        //     2: mutar_discord: ligado    -> desligado   (mas leu "ligado"!)
        //
        // O `Toggle` tinha funcionado na rodada 1; a arvore do Discord e que ainda
        // nao tinha atualizado quando eu li. Um tempo fixo troca "confirmei" por
        // "esperei o bastante na maioria das vezes" — e confirmar e a unica coisa
        // que esta ferramenta tem que a tecla nao tem.
        //
        // Se estourar o prazo sem mudar, `depois` volta igual a `antes` e quem
        // chamou VE isso. Silenciar seria pior que o problema original.
        const PASSO: std::time::Duration = std::time::Duration::from_millis(60);
        const TETO: u32 = 40; // ~2,4 s
        let mut depois = antes;
        for _ in 0..TETO {
            std::thread::sleep(PASSO);
            let mut agora = -1;
            estado(pat, &mut agora);
            if agora != antes {
                depois = agora;
                break;
            }
        }
        unsafe { soltar(pat) };
        Ok(Resultado { antes, depois })
    }
}

#[cfg(windows)]
pub use imp::{alternar, Resultado};

#[cfg(not(windows))]
pub struct Resultado {
    pub antes: i32,
    pub depois: i32,
}

#[cfg(not(windows))]
pub fn alternar(_janela: &str, _controle: &str) -> Result<Resultado, String> {
    Err("UIAutomation so existe no Windows".into())
}

/// 0 desligado, 1 ligado, 2 indefinido — o vocabulário do `ToggleState`.
pub fn rotulo(estado: i32) -> &'static str {
    match estado {
        0 => "desligado",
        1 => "ligado",
        _ => "indefinido",
    }
}

//! Achar um controle **pelo nome** e acioná-lo, sem enxergar a tela.
//!
//! ## Por que isto existe
//!
//! `Ctrl+Shift+M` alterna o mudo do Discord às cegas: não sabe se ficou mudo, e
//! depende do atalho global estar configurado. Tecla de mídia pula faixa mas não
//! sabe **tocar uma música pelo nome**. O UIAutomation — a mesma API que leitor de
//! tela usa — resolve os dois: lê o estado, age, e confirma.
//!
//! ## Achar a janela: por PROCESSO, não por título
//!
//! Custou duas medições falhadas para eu entender. No Discord o título contém
//! "Discord" e casar por texto funciona. No Spotify **o título É a música tocando**:
//!
//! ```text
//! parado     "Spotify Free"
//! tocando    "Shiny_sz - Rota da Ira"
//! 3 min dps  "Gibran Alcocer - Idea 22"
//! ```
//!
//! Procurar "Spotify" no título falha exatamente quando ele está fazendo o que se
//! quer controlar. Por isso um alvo terminado em `.exe` casa pelo **executável**
//! (`GetWindowThreadProcessId` + `QueryFullProcessImageNameW`), que não muda.
//!
//! ## O que cada aplicativo publica — medido, e eu errei antes
//!
//! ```text
//! Discord  (Electron)   898 controles
//! Spotify  (Electron)   947 controles   <- ver nota
//! WhatsApp (WebView2)  1238
//! XBOX     (UWP)         12
//! Radmin   (Win32)        0
//! ```
//!
//! **A nota importa.** Numa primeira medição o Spotify deu 15 controles e eu
//! concluí "não serve para UIA". Estava errado: a janela estava num estado sem o
//! renderizador carregado. Com ele tocando, são 947 — a árvore inteira, com um
//! botão `Tocar <faixa> de <artista>` por linha visível. Conclusão tirada de uma
//! amostra ruim é pior que nenhuma conclusão, porque fecha a porta.
//!
//! ## Armadilhas de COM que custaram tentativa
//!
//! 1. **Errar índice de vtable não dá erro de compilação.** Cada índice aqui foi
//!    validado sozinho: `GetRootElement`[5] + `get_CurrentName`[23] devolveram
//!    "Área de Trabalho 1", que é o nome real do elemento raiz.
//! 2. **`GetCurrentPattern` devolve S_OK com ponteiro NULO** quando o controle não
//!    suporta o padrão. Testar só o `HRESULT` passa batido.
//! 3. **O botão de mudo é `Toggle`, não `Invoke`.** O de tocar faixa é `Invoke`.
//!    Sondar antes de assumir.
//! 4. **Nome igual, tipo diferente.** `Tocar From The Start de Laufey` casa com a
//!    LINHA da tabela (50029) e com o BOTÃO (50000). Acionar a linha não faz nada.

#![allow(non_snake_case)]

/// Tipos de controle do UIAutomation que este módulo usa.
pub const TIPO_BOTAO: i32 = 50000;
pub const TIPO_EDICAO: i32 = 50004;
/// O campo de busca do Spotify e um ComboBox, nao um Edit — descoberto errando:
/// procurar so por `Edit` nao achava "O que você quer ouvir?".
pub const TIPO_COMBO: i32 = 50003;

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::sync::Mutex;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid { d1: u32, d2: u16, d3: u16, d4: [u8; 8] }
    const CLSID_CUIAUTOMATION: Guid = Guid {
        d1: 0xFF48DBA4, d2: 0x60EF, d3: 0x4201,
        d4: [0xAA, 0x87, 0x54, 0x10, 0x3E, 0xEF, 0x59, 0x4E],
    };
    const IID_IUIAUTOMATION: Guid = Guid {
        d1: 0x30CBE57D, d2: 0xD9D0, d3: 0x452A,
        d4: [0xAB, 0x13, 0x7A, 0xC5, 0xAC, 0x48, 0x25, 0xEE],
    };
    const PATTERN_INVOKE: i32 = 10000;
    const PATTERN_TOGGLE: i32 = 10015;
    const PATTERN_VALOR: i32 = 10002;
    const ESCOPO_DESCENDENTES: i32 = 4;

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
        fn GetForegroundWindow() -> isize;
        fn SetForegroundWindow(h: isize) -> i32;
                fn AttachThreadInput(de: u32, para: u32, liga: i32) -> i32;
        fn EnumWindows(f: extern "system" fn(isize, isize) -> i32, p: isize) -> i32;
        fn GetWindowTextW(h: isize, b: *mut u16, n: i32) -> i32;
        fn IsWindowVisible(h: isize) -> i32;
        fn GetWindowThreadProcessId(h: isize, pid: *mut u32) -> u32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThreadId() -> u32;
        fn OpenProcess(acesso: u32, herdar: i32, pid: u32) -> isize;
        fn QueryFullProcessImageNameW(h: isize, f: u32, b: *mut u16, n: *mut u32) -> i32;
        fn CloseHandle(h: isize) -> i32;
    }

    const SEP: [char; 2] = ['\\', '/'];

    /// Nome do executável dono da janela. **Estável**, ao contrário do título.
    fn exe_da_janela(h: isize) -> String {
        unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, &mut pid);
            if pid == 0 { return String::new(); }
            // PROCESS_QUERY_LIMITED_INFORMATION: o minimo para ler o caminho, e o
            // unico que funciona sem privilegio elevado.
            let ph = OpenProcess(0x1000, 0, pid);
            if ph == 0 { return String::new(); }
            let mut buf = [0u16; 512];
            let mut n = 512u32;
            let ok = QueryFullProcessImageNameW(ph, 0, buf.as_mut_ptr(), &mut n);
            CloseHandle(ph);
            if ok == 0 { return String::new(); }
            String::from_utf16_lossy(&buf[..n as usize])
                .rsplit(SEP)
                .next()
                .unwrap_or("")
                .to_string()
        }
    }

    static ALVO: Mutex<isize> = Mutex::new(0);
    static PROCURA: Mutex<String> = Mutex::new(String::new());

    extern "system" fn visitar(h: isize, _: isize) -> i32 {
        if unsafe { IsWindowVisible(h) } == 0 { return 1; }
        let spec = PROCURA.lock().unwrap().clone();
        let casa = if spec.to_lowercase().ends_with(".exe") {
            exe_da_janela(h).eq_ignore_ascii_case(&spec)
        } else {
            let mut b = [0u16; 256];
            let n = unsafe { GetWindowTextW(h, b.as_mut_ptr(), 256) };
            n > 0 && String::from_utf16_lossy(&b[..n as usize]).contains(spec.as_str())
        };
        if casa { *ALVO.lock().unwrap() = h; return 0; }
        1
    }

    /// Primeira janela visível que case com `spec`.
    ///
    /// `spec` terminado em `.exe` casa pelo executável; qualquer outra coisa casa
    /// por trecho do título. Ver a nota do módulo sobre por que os dois existem.
    pub fn janela_de(spec: &str) -> Option<isize> {
        janela(spec)
    }

    fn janela(spec: &str) -> Option<isize> {
        *PROCURA.lock().unwrap() = spec.to_string();
        *ALVO.lock().unwrap() = 0;
        unsafe { EnumWindows(visitar, 0) };
        let h = *ALVO.lock().unwrap();
        (h != 0).then_some(h)
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    unsafe fn bstr(p: *mut u16) -> String {
        if p.is_null() { return String::new(); }
        let n = unsafe { *(p as *const u32).offset(-1) } as usize;
        let s = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, n / 2) });
        unsafe { SysFreeString(p) };
        s
    }
    unsafe fn vt(o: *mut c_void, i: usize) -> *const c_void {
        unsafe { *(*(o as *const *const *const c_void)).add(i) }
    }
    unsafe fn soltar(o: *mut c_void) {
        if !o.is_null() {
            let r: extern "system" fn(*mut c_void) -> u32 = unsafe { std::mem::transmute(vt(o, 2)) };
            r(o);
        }
    }
    unsafe fn nome_de(el: *mut c_void) -> String {
        let g: extern "system" fn(*mut c_void, *mut *mut u16) -> i32 =
            unsafe { std::mem::transmute(vt(el, 23)) };
        let mut s = std::ptr::null_mut();
        g(el, &mut s);
        unsafe { bstr(s) }
    }
    unsafe fn tipo_de(el: *mut c_void) -> i32 {
        let g: extern "system" fn(*mut c_void, *mut i32) -> i32 =
            unsafe { std::mem::transmute(vt(el, 21)) };
        let mut t = 0;
        g(el, &mut t);
        t
    }

    /// Devolve o foco a quem estava na frente, ao sair de escopo.
    ///
    /// **O John joga em tela cheia.** Medido: acionar o botao de mudo do Discord ou
    /// tocar uma faixa no Spotify TRAZ A JANELA PARA A FRENTE — o UIAutomation nao
    /// manda clique, mas o aplicativo reage subindo. No meio de uma partida isso
    /// tira ele do jogo, que e exatamente o que a ferramenta existe para evitar.
    ///
    /// `SetForegroundWindow` sozinho costuma ser recusado pelo Windows. O truque de
    /// `AttachThreadInput` — ligar a fila de entrada da nossa thread a da janela
    /// alvo — e o que faz o sistema aceitar. Ligar e desligar em volta da chamada.
    struct GuardaFoco { anterior: isize }

    impl GuardaFoco {
        fn novo() -> Self {
            Self { anterior: unsafe { GetForegroundWindow() } }
        }
    }

    impl Drop for GuardaFoco {
        fn drop(&mut self) {
            if self.anterior == 0 { return; }
            // DEVOLVE O QUE DER, e nao promete mais que isso.
            //
            // Medido, e o resultado e INCONSISTENTE entre rodadas: as vezes volta,
            // as vezes o aplicativo sobe de novo depois. Nao e bug meu — o Windows
            // impede de proposito que um processo de fundo tome o primeiro plano, e
            // o `AttachThreadInput` so as vezes contorna.
            //
            // **Consequencia pratica, e ela importa:** para quem esta jogando em
            // tela cheia, o caminho por UIA NAO e seguro. O caminho quieto e a tecla
            // (`Como::Teclas`), que age sem tocar em janela nenhuma. UIA vale quando
            // confirmar o estado importa mais que nao ser interrompido.
            for _ in 0..3 {
                unsafe {
                    if GetForegroundWindow() == self.anterior { return; }
                    let mut pid = 0u32;
                    let alheia = GetWindowThreadProcessId(self.anterior, &mut pid);
                    let minha = GetCurrentThreadId();
                    if alheia != 0 && alheia != minha {
                        AttachThreadInput(minha, alheia, 1);
                        SetForegroundWindow(self.anterior);
                        AttachThreadInput(minha, alheia, 0);
                    } else {
                        SetForegroundWindow(self.anterior);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
        }
    }

    /// Guarda o foco por uma OPERACAO INTEIRA, e nao por sessao.
    ///
    /// `tocar_faixa` abre tres sessoes (escrever, listar, acionar). Com so a guarda
    /// por sessao, a primeira rouba o foco para o Spotify e a ultima "devolve" para
    /// o Spotify — que ja era o estado errado. Medido: o Discord voltava certo e o
    /// Spotify ficava na frente.
    ///
    /// Quem chama uma operacao de varios passos segura isto do comeco ao fim.
    pub struct Foco(GuardaFoco);

    impl Foco {
        pub fn guardar() -> Self { Foco(GuardaFoco::novo()) }
    }

    /// Sessão de automação viva: o objeto UIA e o elemento da janela.
    ///
    /// O `foco` fica DEPOIS dos ponteiros COM de proposito: `Drop` roda em ordem de
    /// declaracao, entao o foco volta por ultimo — depois de a acao ter terminado.
    struct Sessao { aut: *mut c_void, jan: *mut c_void, _foco: GuardaFoco }

    impl Drop for Sessao {
        fn drop(&mut self) {
            unsafe { soltar(self.jan); soltar(self.aut) };
        }
    }

    fn abrir(spec: &str) -> Result<Sessao, String> {
        let Some(hwnd) = janela(spec) else {
            return Err(format!(
                "nao achei janela visivel de {spec:?} (o app esta fechado ou na bandeja?)"
            ));
        };
        unsafe {
            let hr = CoInitializeEx(std::ptr::null_mut(), 2);
            if hr < 0 { return Err(format!("CoInitializeEx falhou (0x{hr:08X})")); }
            let mut aut: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_CUIAUTOMATION, std::ptr::null_mut(), 1, &IID_IUIAUTOMATION, &mut aut,
            );
            if hr != 0 || aut.is_null() {
                return Err(format!("UIAutomation indisponivel (0x{hr:08X})"));
            }
            let f: extern "system" fn(*mut c_void, isize, *mut *mut c_void) -> i32 =
                std::mem::transmute(vt(aut, 6));
            let mut jan: *mut c_void = std::ptr::null_mut();
            if f(aut, hwnd, &mut jan) != 0 || jan.is_null() {
                soltar(aut);
                return Err("nao consegui ler a janela".into());
            }
            Ok(Sessao { aut, jan, _foco: GuardaFoco::novo() })
        }
    }

    /// Varre a árvore e devolve o primeiro controle do tipo pedido cujo nome case.
    ///
    /// Varre em vez de usar `FindFirst` com condição de nome porque o alvo aqui é
    /// **parcial** ("Tocar From The Start" dentro de "Tocar From The Start de
    /// Laufey") e porque o TIPO precisa entrar no filtro: o mesmo nome aparece na
    /// linha da tabela e no botão, e acionar a linha não faz nada.
    /// Varre e diz TAMBEM quantos controles a arvore ofereceu.
    ///
    /// O numero e o que separa "esse controle nao existe" de "a arvore ainda nao
    /// nasceu" — ver `achar_teimoso`.
    unsafe fn achar_contando(s: &Sessao, tipo: i32, trecho: &str) -> (Option<*mut c_void>, i32) {
        let ct: extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 =
            unsafe { std::mem::transmute(vt(s.aut, 21)) };
        let mut cond: *mut c_void = std::ptr::null_mut();
        if ct(s.aut, &mut cond) != 0 { return (None, 0); }

        let fa: extern "system" fn(*mut c_void, i32, *mut c_void, *mut *mut c_void) -> i32 =
            unsafe { std::mem::transmute(vt(s.jan, 6)) };
        let mut arr: *mut c_void = std::ptr::null_mut();
        let hr = fa(s.jan, ESCOPO_DESCENDENTES, cond, &mut arr);
        unsafe { soltar(cond) };
        if hr != 0 || arr.is_null() { return (None, 0); }

        let gl: extern "system" fn(*mut c_void, *mut i32) -> i32 =
            unsafe { std::mem::transmute(vt(arr, 3)) };
        let ge: extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> i32 =
            unsafe { std::mem::transmute(vt(arr, 4)) };
        let mut n = 0;
        gl(arr, &mut n);
        let alvo = trecho.to_lowercase();
        let mut achado = None;
        for i in 0..n {
            let mut el: *mut c_void = std::ptr::null_mut();
            if ge(arr, i, &mut el) != 0 || el.is_null() { continue; }
            let nm = unsafe { nome_de(el) };
            if unsafe { tipo_de(el) } == tipo
                && nm.to_lowercase().contains(&alvo)
                && !proibido(&nm)
            {
                achado = Some(el);
                break;
            }
            unsafe { soltar(el) };
        }
        unsafe { soltar(arr) };
        (achado, n)
    }

    /// ## Por que agir sobre OUTRA PESSOA nunca vai sair daqui
    ///
    /// Nao e cautela: e que a arvore de UI **nao tem identidade**. Medido no
    /// Discord do John, cujo apelido e "Stitch", nos 881 controles da arvore:
    ///
    /// ```text
    /// Pane       'Stitch | R.E.P.O. Brasil - Discord'   a janela
    /// Document   'Stitch | R.E.P.O. Brasil'
    /// Hyperlink  'Stitch (canal de voz), Stitch, ...'
    /// Button     'Stitch'
    /// Text       'Stitch'  (x3)
    /// Text       'Canal de voz Stitch'                  um CANAL com o nome dele
    /// Image      'Stitch'                               o avatar
    /// ```
    ///
    /// Dez controles, de sete tipos, um so e a pessoa. E o casamento por trecho e
    /// escopeta: "an" casa com 9 nomes de participante, e o primeiro que "dash"
    /// encontra e o resumo do canal — que lista todo mundo dentro, entao contem o
    /// nome de todos. Some a isso apelido em unicode estilizado (`𝕿𝖍𝖊𝖔`,
    /// `🍂 𝙵 𝙸 𝙻 𝙸 𝙿 𝙴 🍂`), que `normalizar` reduz a nada.
    ///
    /// **Nome nao e identificador numa arvore de UI, e rotulo — e rotulo se repete
    /// entre coisas de tipos diferentes.** Nao ha casamento melhor que conserte:
    /// a informacao que distingue as pessoas nao esta aqui.
    ///
    /// Ela esta no ID do Discord, que a Nyxara guarda e a arvore nunca publica.
    /// Entao a divisao e:
    ///
    /// ```text
    /// sobre VOCE MESMO      UI serve — seu botao de mudo nao e achado por nome,
    ///                       e estrutural (pai "Status do usuario e configuracoes")
    /// sobre OUTRA PESSOA    so pela API, com ID. Por UI nao e dificil: e impossivel
    /// ```
    ///
    /// Nomes que ela NUNCA aciona sozinha, nem por casamento parcial.
    ///
    /// O John tem cargo de administrador no servidor dele. No Discord isso muda o
    /// que os controles significam:
    ///
    /// ```text
    /// "Silenciar"                    silencia so PARA MIM         — local
    /// "Silenciar voz no servidor"    silencia para TODO MUNDO     — para fora
    /// "Desativar audio no servidor"  idem                          — para fora
    /// "Desconectar"                  expulsa a pessoa da chamada   — para fora
    /// ```
    ///
    /// E `"Silenciar voz no servidor"` **contem** `"Silenciar"`. Casamento por
    /// substring pega o errado, e o errado aqui e um ato publico sobre outra pessoa.
    ///
    /// Hoje esses controles nao estao na arvore (sao item de menu de contexto, so
    /// nascem no clique direito) — medido, 868 controles, nenhum deles la. Isto e
    /// defesa em profundidade, para quando um deles aparecer.
    ///
    /// Nada disto e "a Teka nao pode": e "a Teka nao faz isto CALADA, por um
    /// casamento aproximado de nome". Ato sobre outra pessoa se pede explicitamente.
    const NUNCA_SOZINHA: &[&str] = &[
        "no servidor",
        "desconectar",
        "expulsar",
        "banir",
        "acoes de servidor",
        "ações de servidor",
    ];

    /// Este nome e proibido de acionar?
    fn proibido(nome: &str) -> bool {
        let n = nome.to_lowercase();
        NUNCA_SOZINHA.iter().any(|p| n.contains(p))
    }

    /// Quantos controles a arvore precisa ter para eu acreditar num "nao achei".
    ///
    /// Discord vivo publica 1066; Spotify carregado, 947. Descarregado, o Spotify
    /// da 25. Qualquer coisa abaixo disto e arvore fria, nao ausencia.
    const ARVORE_FRIA: i32 = 60;

    /// Quanto esperar entre as tentativas, quando a arvore parece fria.
    ///
    /// Medido: 5s depois de o Discord subir a arvore ja tinha 1153 controles; no
    /// instante em que a janela aparece, 7. Oito tentativas de 300ms cobrem 2,4s.
    const ESPERA_FRIA_MS: u64 = 300;

    /// Como `achar_teimoso`, mas so aceita o controle que SABE fazer o que eu quero.
    ///
    /// ## Por que o nome nao basta
    ///
    /// O Discord publica DOIS botoes chamados "Silenciar", os dois seus:
    ///
    /// ```text
    /// #1  pai='Status do usuario e configuracoes'  pats=Value,Toggle,ScrollItem
    /// #2  pai=''  (painel da chamada)              pats=Invoke,ScrollItem
    /// ```
    ///
    /// So o #1 alterna. Pegar o primeiro pelo nome funciona hoje por ORDEM DE
    /// ARVORE, nao por desenho — se o Discord reordenar, `alternar` morre num
    /// "esse controle nao e de alternancia" que nao explica nada.
    ///
    /// (Os controles de OUTRAS pessoas nao entram nisto: eles se chamam
    /// "Stitch, Silenciado(a)", com o nome de quem e. O risco aqui nunca foi mutar
    /// o outro — foi achar o botao que nao alterna.)
    unsafe fn achar_que_faz(
        s: &Sessao,
        tipo: i32,
        trecho: &str,
        padrao_id: i32,
        exato: bool,
    ) -> Option<(*mut c_void, *mut c_void)> {
        for tentativa in 0..8 {
            let (todos, n) = unsafe { todos_que_casam(s, tipo, trecho, exato) };
            for el in &todos {
                if let Some(pat) = unsafe { padrao(*el, padrao_id) } {
                    for outro in &todos {
                        if outro != el {
                            unsafe { soltar(*outro) };
                        }
                    }
                    return Some((*el, pat));
                }
            }
            for el in &todos {
                unsafe { soltar(*el) };
            }
            if n >= ARVORE_FRIA {
                return None;
            }
            if tentativa < 7 {
                std::thread::sleep(std::time::Duration::from_millis(ESPERA_FRIA_MS));
            }
        }
        None
    }

    /// Todos os controles do tipo cujo nome contenha o trecho, e o tamanho da arvore.
    unsafe fn todos_que_casam(
        s: &Sessao,
        tipo: i32,
        trecho: &str,
        exato: bool,
    ) -> (Vec<*mut c_void>, i32) {
        let ct: extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 =
            unsafe { std::mem::transmute(vt(s.aut, 21)) };
        let mut cond: *mut c_void = std::ptr::null_mut();
        if ct(s.aut, &mut cond) != 0 { return (Vec::new(), 0); }
        let fa: extern "system" fn(*mut c_void, i32, *mut c_void, *mut *mut c_void) -> i32 =
            unsafe { std::mem::transmute(vt(s.jan, 6)) };
        let mut arr: *mut c_void = std::ptr::null_mut();
        let hr = fa(s.jan, ESCOPO_DESCENDENTES, cond, &mut arr);
        unsafe { soltar(cond) };
        if hr != 0 || arr.is_null() { return (Vec::new(), 0); }
        let gl: extern "system" fn(*mut c_void, *mut i32) -> i32 =
            unsafe { std::mem::transmute(vt(arr, 3)) };
        let ge: extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> i32 =
            unsafe { std::mem::transmute(vt(arr, 4)) };
        let mut n = 0;
        gl(arr, &mut n);
        let alvo = trecho.to_lowercase();
        let mut fora = Vec::new();
        for i in 0..n {
            let mut el: *mut c_void = std::ptr::null_mut();
            if ge(arr, i, &mut el) != 0 || el.is_null() { continue; }
            let nm = unsafe { nome_de(el) };
            let casa = if exato {
                nm.to_lowercase() == alvo
            } else {
                nm.to_lowercase().contains(&alvo)
            };
            if unsafe { tipo_de(el) } == tipo && casa && !proibido(&nm) {
                fora.push(el);
            } else {
                unsafe { soltar(el) };
            }
        }
        unsafe { soltar(arr) };
        (fora, n)
    }

    /// Procura, e se a arvore parecer fria, espera e procura DE NOVO.
    ///
    /// ## O que isto conserta
    ///
    /// A arvore de acessibilidade do Chromium nasce **preguicosa**: ela so e
    /// construida quando um cliente UIA pergunta, e a PRIMEIRA pergunta volta antes
    /// de a construcao terminar. Medido em PowerShell, a mesma consulta duas vezes
    /// seguidas no mesmo Discord:
    ///
    /// ```text
    /// 1a   NAO ACHOU o controle 'Silenciar'   863ms
    /// 2a   ACHOU  nome='Silenciar'            461ms
    /// ```
    ///
    /// Discord e Spotify sao os dois Electron, entao os dois tem isto. Sem a
    /// repeticao, o primeiro "me muta" depois de a Teka subir falha, e o segundo
    /// funciona — o pior tipo de defeito, porque parece instabilidade.
    ///
    /// A contagem e o que evita transformar todo erro honesto em 2,4s de espera:
    /// arvore cheia sem o controle e ausencia de verdade, e desiste na hora.
    unsafe fn achar_teimoso(s: &Sessao, tipo: i32, trecho: &str) -> Option<*mut c_void> {
        for tentativa in 0..4 {
            let (achado, n) = unsafe { achar_contando(s, tipo, trecho) };
            if achado.is_some() || n >= ARVORE_FRIA {
                return achado;
            }
            if tentativa < 3 {
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
        }
        None
    }

    unsafe fn padrao(el: *mut c_void, id: i32) -> Option<*mut c_void> {
        let gp: extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> i32 =
            unsafe { std::mem::transmute(vt(el, 16)) };
        let mut p: *mut c_void = std::ptr::null_mut();
        // S_OK com ponteiro NULO quando o padrao nao existe — ver nota do modulo.
        if gp(el, id, &mut p) != 0 || p.is_null() { None } else { Some(p) }
    }

    pub struct Resultado { pub antes: i32, pub depois: i32 }

    /// Alterna um controle de dois estados (mudo, ensurdecer) e **confirma**.
    pub fn alternar(spec: &str, controle: &str) -> Result<Resultado, String> {
        let s = abrir(spec)?;
        unsafe {
            // O que eu quero nao e "um botao com esse nome", e "um botao com esse
            // nome QUE ALTERNA". Ver `achar_que_faz`: o Discord publica dois
            // "Silenciar" e so um deles tem Toggle.
            // NOME EXATO aqui, nao substring: "Silenciar voz no servidor" contem
            // "Silenciar", e sao coisas diferentes — uma e sobre mim, a outra e um
            // ato publico sobre outra pessoa. Ver `NUNCA_SOZINHA`.
            let (el, pat) = achar_que_faz(&s, super::TIPO_BOTAO, controle, PATTERN_TOGGLE, true)
                .ok_or_else(|| format!("nao achei botao {controle:?} que saiba alternar"))?;
            let estado: extern "system" fn(*mut c_void, *mut i32) -> i32 =
                std::mem::transmute(vt(pat, 4));
            let acionar: extern "system" fn(*mut c_void) -> i32 = std::mem::transmute(vt(pat, 3));

            let mut antes = -1;
            estado(pat, &mut antes);
            let hr = acionar(pat);
            if hr != 0 {
                soltar(pat); soltar(el);
                return Err(format!("Toggle falhou (0x{hr:08X})"));
            }
            // ESPERA ATE MUDAR, e nao um tempo fixo.
            //
            // A versao com 400 ms fixos reportou "desligado -> desligado" numa
            // rodada que TINHA mudado (a seguinte leu "ligado"). Tempo fixo troca
            // "confirmei" por "esperei o bastante quase sempre" — e confirmar e a
            // unica coisa que isto tem que a tecla nao tem.
            let mut depois = antes;
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(60));
                let mut agora = -1;
                estado(pat, &mut agora);
                if agora != antes { depois = agora; break; }
            }
            soltar(pat); soltar(el);
            Ok(Resultado { antes, depois })
        }
    }

    /// Aciona um botão pelo nome (parcial). Devolve o nome inteiro do que acionou.
    pub fn acionar_botao(spec: &str, trecho: &str) -> Result<String, String> {
        let s = abrir(spec)?;
        unsafe {
            if proibido(trecho) {
                return Err(format!("{trecho:?} e ato sobre outra pessoa; isso nao sai de atalho"));
            }
            let el = achar_teimoso(&s, super::TIPO_BOTAO, trecho)
                .ok_or_else(|| format!("nao achei botao contendo {trecho:?}"))?;
            let nome = nome_de(el);
            let Some(pat) = padrao(el, PATTERN_INVOKE) else {
                soltar(el);
                return Err(format!("{nome:?} nao e acionavel (sem padrao Invoke)"));
            };
            let inv: extern "system" fn(*mut c_void) -> i32 = std::mem::transmute(vt(pat, 3));
            let hr = inv(pat);
            soltar(pat); soltar(el);
            if hr != 0 { return Err(format!("Invoke falhou (0x{hr:08X})")); }
            Ok(nome)
        }
    }

    /// Escreve num campo de edição (o campo de busca, por exemplo).
    pub fn escrever(spec: &str, campo: &str, texto: &str) -> Result<(), String> {
        let s = abrir(spec)?;
        unsafe {
            // Edit OU ComboBox: o campo de busca do Spotify e o segundo.
            let el = achar_teimoso(&s, super::TIPO_EDICAO, campo)
                .or_else(|| achar_teimoso(&s, super::TIPO_COMBO, campo))
                .ok_or_else(|| format!("nao achei campo contendo {campo:?}"))?;
            let Some(pat) = padrao(el, PATTERN_VALOR) else {
                soltar(el);
                return Err("esse campo nao aceita escrita (sem padrao Value)".into());
            };
            // IUIAutomationValuePattern [3] = SetValue(BSTR)
            let sv: extern "system" fn(*mut c_void, *mut u16) -> i32 =
                std::mem::transmute(vt(pat, 3));
            let w = wide(texto);
            let b = SysAllocString(w.as_ptr());
            let hr = sv(pat, b);
            SysFreeString(b);
            soltar(pat); soltar(el);
            if hr != 0 { return Err(format!("nao consegui escrever (0x{hr:08X})")); }
            Ok(())
        }
    }

    /// Lista os controles de um tipo cujo nome contenha `trecho`. Para diagnóstico.
    /// Lista os controles de um tipo cujo nome contenha o trecho. So leitura.
    ///
    /// Teimosa pela mesma razao de `achar_teimoso`: numa arvore fria ela devolveria
    /// lista vazia e a pessoa concluiria que o aplicativo nao publica nada. Foi
    /// exatamente o erro que eu ja cometi com o Spotify — 15 controles medidos com
    /// o renderizador descarregado, 947 com ele de pe.
    pub fn listar(spec: &str, tipo: i32, trecho: &str) -> Result<Vec<String>, String> {
        for tentativa in 0..4 {
            let (fora, n) = listar_uma_vez(spec, tipo, trecho)?;
            if !fora.is_empty() || n >= ARVORE_FRIA {
                return Ok(fora);
            }
            if tentativa < 3 {
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
        }
        Ok(Vec::new())
    }

    /// Uma varredura so, sem teimosia — e o que enxerga a arvore FRIA.
    ///
    /// Publica de proposito: e o instrumento que mede o fenomeno. Ferramenta
    /// nenhuma chama isto; quem chama e `listar`, e a sonda `sonda_uia`.
    pub fn listar_uma_vez(spec: &str, tipo: i32, trecho: &str) -> Result<(Vec<String>, i32), String> {
        let s = abrir(spec)?;
        let mut fora = Vec::new();
        let mut total = 0i32;
        unsafe {
            let ct: extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 =
                std::mem::transmute(vt(s.aut, 21));
            let mut cond: *mut c_void = std::ptr::null_mut();
            ct(s.aut, &mut cond);
            let fa: extern "system" fn(*mut c_void, i32, *mut c_void, *mut *mut c_void) -> i32 =
                std::mem::transmute(vt(s.jan, 6));
            let mut arr: *mut c_void = std::ptr::null_mut();
            fa(s.jan, ESCOPO_DESCENDENTES, cond, &mut arr);
            soltar(cond);
            if arr.is_null() { return Ok((fora, 0)); }
            let gl: extern "system" fn(*mut c_void, *mut i32) -> i32 = std::mem::transmute(vt(arr, 3));
            let ge: extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> i32 =
                std::mem::transmute(vt(arr, 4));
            let mut n = 0;
            gl(arr, &mut n);
            total = n;
            let alvo = trecho.to_lowercase();
            for i in 0..n {
                let mut el: *mut c_void = std::ptr::null_mut();
                if ge(arr, i, &mut el) != 0 || el.is_null() { continue; }
                let nm = nome_de(el);
                if (tipo == 0 || tipo_de(el) == tipo) && nm.to_lowercase().contains(&alvo) {
                    fora.push(nm);
                }
                soltar(el);
            }
            soltar(arr);
        }
        Ok((fora, total))
    }
}

#[cfg(windows)]
pub use imp::{acionar_botao, alternar, escrever, janela_de, listar, listar_uma_vez, Foco, Resultado};

#[cfg(not(windows))]
pub struct Resultado { pub antes: i32, pub depois: i32 }
#[cfg(not(windows))]
pub struct Foco;
#[cfg(not(windows))]
impl Foco { pub fn guardar() -> Self { Foco } }
#[cfg(not(windows))]
pub fn alternar(_s: &str, _c: &str) -> Result<Resultado, String> { Err(so_windows()) }
#[cfg(not(windows))]
pub fn acionar_botao(_s: &str, _t: &str) -> Result<String, String> { Err(so_windows()) }
#[cfg(not(windows))]
pub fn escrever(_s: &str, _c: &str, _t: &str) -> Result<(), String> { Err(so_windows()) }
#[cfg(not(windows))]
pub fn listar(_s: &str, _t: i32, _x: &str) -> Result<Vec<String>, String> { Err(so_windows()) }
#[cfg(not(windows))]
fn so_windows() -> String { "UIAutomation so existe no Windows".into() }

/// 0 desligado, 1 ligado, 2 indefinido — o vocabulário do `ToggleState`.
pub fn rotulo(estado: i32) -> &'static str {
    match estado {
        0 => "desligado",
        1 => "ligado",
        _ => "indefinido",
    }
}

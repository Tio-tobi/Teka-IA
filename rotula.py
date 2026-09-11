# -*- coding: utf-8 -*-
"""Rotula as frases do John: ferramenta + argumento copiavel do proprio pedido.

REGRA UNICA, e ela decide quase tudo: a cabeca de ponteiro COPIA um trecho do
pedido. Se o argumento obrigatorio nao esta escrito ali, nao existe resposta certa
possivel -- e o rotulo e `perguntar`.
"""
import io, re, random, unicodedata

def sem(s):
    s = unicodedata.normalize('NFD', s.lower())
    return ''.join(c for c in s if unicodedata.category(c) != 'Mn')

# chave: um trecho distintivo da frase -> argumento esperado (ou None = perguntar)
ARG = {
 # buscar_web -- a consulta, sem o verbo de busca
 "reutilizar um html":        "como que faz pra reutilizar um html no python",
 "qual jogo vai":             "qual jogo vai lançar na steam esse mês",
 "api em python do zero":     "como eu faço uma api em python do zero",
 "instalar o node":           "como instalar o node no windows",
 "sqlite e postgresql":       "qual a diferença entre sqlite e postgresql",
 "bot do discord tocar":      "como que faz um bot do discord tocar musica",
 "rx 580":                    "quanto tá uma rx 580 usada hoje",
 "minecraft novo":            "alguma notícia do minecraft novo",
 "cors no express":           "erro de cors no express",
 "funciona o ollama":         "como funciona o ollama no windows",
 "html css e javascript":     "como fazer um site com html css e javascript",
 "curso bom de python":       "algum curso bom de python pra iniciante",
 # abrir_programa
 "o vscode ai":               "vscode",
 "o navegador pra mim":       "navegador",
 "o discord ai":              "discord",
 "abre o terminal":           "terminal",
 "o spotify":                 "spotify",
 "abrir o python ai":         "python",
 "explorador de arquivos":    "explorador de arquivos",
 "inicia o lm studio":        "lm studio",
 "liga o ollama":             "ollama",
 "bloco de notas":            "bloco de notas",
 "vscode que eu quero":       "vscode",
 "abre o navegador ai":       "navegador",
 # copiar_arquivo
 "relatório da área de trabalho pra documentos": "documentos",
 "cópia desse arquivo na pasta backup":          "backup",
 "arquivo do estágio pra pasta documentos":      "documentos",
 "esse arquivo pra unidade E":                   "unidade E",
 "faz um backup desse txt":                      None,   # "backup" e substantivo, nao destino
 # mover_arquivo
 "joga na pasta documentos":  "documentos",
 "pra pasta do estágio":      "estágio",
 "coloca na pasta fotos":     "fotos",
 "manda pra documentos":      "documentos",
 "negócio pra backup":        "backup",
 "essa pasta pra unidade E":  "unidade E",
 # criar_pasta
 "pasta chamada projetos":    "projetos",
 "pasta nova na área de trabalho": None,   # pasta sem NOME: subespecificado
 "pasta de backup ai":        "backup",
 "chamada estudos":           "estudos",
 "pasta chamada teste":       "teste",
 "chamada imagens":           "imagens",
 "chamada arquivos antigos":  "arquivos antigos",
 "chamada escola":            "escola",
 "chamada nyxara":            "nyxara",
 # apagar_arquivo
 "arquivo chamado teste":     "teste",
 "apaga o backup velho":      None,   # "o backup velho" nao e nome de arquivo
 # buscar_no_conteudo
 "onde aparece nyxara":       "nyxara",
 "escrito banco de dados":    "banco de dados",
 "erro de cors dentro":       "erro de cors",
 "onde fala de estágio":      "estágio",
 "por localhost":             "localhost",
 "fala de sqlite":            "sqlite",
 "a palavra senha":           "senha",
 "calcularMedia":             "calcularMedia",
 "por import discord":        "import discord",
 "escrito relatório final":   "relatório final",
 "função chamada main":       "main",
 "se tem esse texto":         None,   # "esse texto" nao e padrao
 # escrever_arquivo
 "chamado teste.txt":         "teste.txt",
 "num arquivo na área de trabalho": None,  # nem nome nem texto
 "chamado notas.txt":         "notas.txt",
 "no arquivo config.json":    "config.json",
 # executar_comando
 "o comando dir ai":          "dir",
 "python --version":          "python --version",
 "roda ipconfig":             "ipconfig",
 "executa esse comando aqui": None,
 "npm install":               "npm install",
 "pip install requests":      "pip install requests",
 "pra ver os arquivos da pasta": None,   # nao ha comando literal pra copiar
 "o git status":              "git status",
 "o python pelo terminal":    "python",
 "ollama list":               "ollama list",
 "negócio aqui no cmd":       None,
 "executa npm start":         "npm start",
}

# Quais chaves pertencem a qual bloco -- montado da ordem de `ARG`, que segue os
# blocos do arquivo do John.
_ORDEM = ["buscar_web"]*12 + ["abrir_programa"]*12 + ["copiar_arquivo"]*5 +          ["mover_arquivo"]*6 + ["criar_pasta"]*9 + ["apagar_arquivo"]*2 +          ["buscar_no_conteudo"]*12 + ["escrever_arquivo"]*4 + ["executar_comando"]*12
assert len(_ORDEM) == len(ARG), f"{len(_ORDEM)} != {len(ARG)}"
DO_BLOCO = {}
for _b, _k in zip(_ORDEM, ARG):
    DO_BLOCO.setdefault(_b, []).append(_k)


def carregar():
    fs=[]; b=None
    for l in io.open("dados/frases_john_novas.txt", encoding="utf-8"):
        l=l.rstrip()
        if l.startswith("## "): b=l[3:].split("[")[0].strip(); continue
        if not l or l.startswith("#"): continue
        fs.append((b,l))
    return fs

def desprefixa(f, rng):
    m=re.match(r'^(mano,\s*)?Teka[,]?\s+', f)
    if not m or rng.random()<0.15: return f
    r=f[m.end():]
    return (m.group(1) or "")+(r[0].lower()+r[1:] if r else "")

gat={}
for l in io.open("dados/gatilhos.txt", encoding="utf-8"):
    if "|" not in l or l.strip().startswith("#"): continue
    n,fsx=l.split("|",1)
    for g in fsx.split(","): gat[sem(g.strip())]=n.strip().rstrip("*")

CAM = re.compile(r'\.[a-z]{2,4}\b|[\/]|\bunidade [a-z]\b|\barea de trabalho\b|\bdocumentos\b|\bbackup\b|\bfotos\b|chamad[oa] \w+|\bpasta do est[aá]gio\b')
PRECISA = {"copiar_arquivo","mover_arquivo","info_arquivo","apagar_arquivo","ler_imagem","criar_pasta","escrever_arquivo"}

def rotular():
    rng=random.Random(2026)
    out=[]
    for bloco, f0 in carregar():
        f = desprefixa(f0, rng)
        s = sem(f)
        if bloco.startswith("??"):
            out.append(("perguntar", f, "", "vago")); continue
        if bloco in ("processos","rede"):
            out.append((bloco, f, "", "sem parametro")); continue
        if bloco == "atalho":
            achou=[g for g in gat if g in s]
            if achou:
                g=max(achou,key=len); i=s.find(g)
                out.append((bloco, f, f[i:i+len(g)], "gatilho")); continue
            out.append(("SEM_GATILHO", f, "", "nao ha gatilho na tabela")); continue
        if bloco in PRECISA and not CAM.search(s):
            out.append(("perguntar", f, "", "deitica: pede caminho, nao da caminho")); continue
        # POR BLOCO, e nao global. A primeira versao procurava a chave em toda a
        # tabela e "arquivo chamado teste" (de `apagar_arquivo`) venceu
        # "chamado teste.txt" (de `escrever_arquivo`) por ser mais longa --
        # devolvendo `teste` onde o certo era `teste.txt`.
        achou=[k for k in DO_BLOCO.get(bloco,()) if sem(k) in s]
        if not achou:
            out.append(("??", f, "", f"NAO ROTULADA (bloco {bloco})")); continue
        a = ARG[max(achou, key=len)]
        if a is None:
            out.append(("perguntar", f, "", "argumento nao esta escrito no pedido")); continue
        if a not in f:
            out.append(("??", f, a, "ARGUMENTO NAO E TRECHO EXATO")); continue
        out.append((bloco, f, a, "ok"))
    return out

if __name__ == "__main__":
    import collections, sys
    o = rotular()
    c = collections.Counter(t for t,_,_,_ in o)
    print(f"{len(o)} frases\n")
    for k,v in c.most_common(): print(f"  {k:<20} {v}")
    ruins=[x for x in o if x[0] in ("??","SEM_GATILHO")]
    if ruins:
        print(f"\n{len(ruins)} precisam de atencao:")
        for t,f,a,n in ruins: print(f"   [{n}] {f}" + (f"  (esperado {a!r})" if a else ""))

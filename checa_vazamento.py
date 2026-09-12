# -*- coding: utf-8 -*-
"""O treino esta ensinando o gabarito do benchmark?

RODE ANTES de acrescentar molde ou valor de poco.

A olho nao funciona -- eu tentei em 2026-09-10 e errei. Comparei os verbos novos
contra a lista de ERROS da guarda, que so mostra o que ela FALHA; `listar_pasta` tem
14 frases no benchmark e eu nunca tinha visto nove delas. Seis dos quinze moldes que
escrevi usavam verbo que o benchmark cobra, e quatro deles para OUTRA ferramenta.

Dois tipos de vazamento, e o segundo e pior:

  DIRETO     molde de `listar_pasta` usa verbo que o benchmark cobra para
             `listar_pasta`. Sobe o numero sem subir a capacidade -- o benchmark
             para de medir generalizacao e vira prova com gabarito.

  CRUZADO    molde de `listar_pasta` usa verbo que o benchmark cobra para
             `ler_arquivo`. Isso nao so infla: ROUBA da vizinha. Piora de verdade.

    python checa_vazamento.py exibe percorre   <- O MODO QUE FUNCIONA
    python checa_vazamento.py                  <- varredura, so orientativa

O MODO CONSULTA e confiavel: pergunta "esta palavra ja e usada pelo benchmark, e
para qual ferramenta?". Foi ele que pegou os seis moldes ruins.

A VARREDURA super-reporta e NAO deve virar teste como esta. A primeira versao
marcou 350 ocorrencias e quase todas eram falso alarme -- `abre` aparece em molde de
`abrir_programa` e em frase de `ler_arquivo` porque e verbo comum do portugues. Com
filtro de raridade caiu para 93, e as que sobram ainda incluem ambiguidade legitima
entre ferramentas parecidas (`executa` entre `abrir_programa` e `executar_comando`).

Separar "ambiguidade que ela deve aprender" de "gabarito que eu vazei" exige
julgamento sobre o par de ferramentas, e eu nao sei automatizar isso ainda. Entao a
varredura serve para OLHAR, nao para reprovar.
"""
import io, re, sys, collections

VAZIAS = {"a","o","as","os","um","uma","de","do","da","dos","das","em","no","na",
          "nos","nas","e","que","me","meu","minha","pra","para","por","com","se",
          "ai","la","quero","preciso","qual","quais","tem","ha","esta","ver","faz",
          "pode","mais","tudo","todo","toda","isso","aqui","ao","dentro","sobre"}

quantas = collections.Counter()

def benchmark():
    """palavra -> {ferramentas que a usam}"""
    idx = collections.defaultdict(set)
    for l in io.open("dados/frases_teste.txt", encoding="utf-8"):
        l = l.strip()
        if not l or l.startswith("#") or "|" not in l:
            continue
        p = [x.strip() for x in l.split("|")]
        for w in re.findall(r"[a-zà-ú]+", p[1].lower()):
            if w not in VAZIAS and len(w) > 3:
                idx[w].add(p[0])
        for w in set(re.findall(r"[a-zà-ú]+", p[1].lower())):
            quantas[w] += 1
    return idx

def moldes():
    """(ferramenta, frase) dos moldes do gerador"""
    s = io.open("src/learn/dados.rs", encoding="utf-8").read()
    fora = []
    for m in re.finditer(r'ferramenta:\s*"([a-z_]+)",\s*\n\s*(?://[^\n]*\n\s*)*frases:\s*&\[(.*?)\n\s*\],', s, re.S):
        f = m.group(1)
        corpo = re.sub(r'//[^\n]*', '', m.group(2))   # comentario nao e molde
        for fr in re.findall(r'"([^"]+)"', corpo):
            fora.append((f, fr))
    return fora

idx = benchmark()

if len(sys.argv) > 1:
    for w in sys.argv[1:]:
        donos = idx.get(w.lower())
        print(f"  {w:<16} {'LIVRE' if not donos else 'usado pelo benchmark em: ' + ', '.join(sorted(donos))}")
    raise SystemExit

ms = moldes()

# SO PALAVRA RARA. A primeira versao marcou 350 "cruzados" e quase todos eram
# falso alarme: `abre` aparece em molde de `abrir_programa` e em frase de
# `ler_arquivo` do benchmark porque e verbo comum do portugues. Isso nao e
# vazamento -- e ambiguidade natural, que ela TEM de aprender a desambiguar pelo
# argumento.
#
# O risco de verdade e a palavra DISTINTIVA: `exibe`, `percorre`, `vasculha`. Se
# ela aparece em poucos moldes e em poucas frases do benchmark, ela carrega quase
# sozinha a decisao -- e ai copiar vira gabarito.
freq = collections.Counter()
for _, fr in ms:
    for w in set(re.findall(r"[a-zà-ú]+", re.sub(r"\{\d\}", " ", fr).lower())):
        freq[w] += 1

RARA_NO_MOLDE = 4     # aparece em ate 4 moldes
RARA_NO_BENCH = 2     # e em ate 2 frases do benchmark

direto, cruzado = [], []
for f, fr in ms:
    for w in re.findall(r"[a-zà-ú]+", re.sub(r"\{\d\}", " ", fr).lower()):
        if w in VAZIAS or len(w) <= 3:
            continue
        donos = idx.get(w)
        if not donos or freq[w] > RARA_NO_MOLDE or quantas[w] > RARA_NO_BENCH:
            continue
        (direto if f in donos else cruzado).append((f, w, sorted(donos), fr))

print(f"{len(ms)} moldes lidos, {len(idx)} palavras distintas no benchmark\n")
print(f"CRUZADO ({len(cruzado)}) — molde de uma ferramenta usa palavra que o benchmark")
print( "         cobra para OUTRA. Estes roubam da vizinha; sao os que importam.\n")
for f, w, donos, fr in sorted(cruzado)[:40]:
    print(f"  {f:<20} {w:<14} benchmark usa para: {', '.join(donos)}")
    print(f"  {'':<20} molde: {fr}")
print(f"\nDIRETO ({len(direto)}) — mesma ferramenta dos dois lados. Inflam o numero.")
for f, w, _, fr in sorted(direto)[:25]:
    print(f"  {f:<20} {w:<14} molde: {fr}")

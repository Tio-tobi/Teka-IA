# -*- coding: utf-8 -*-
"""Mede a camada de contexto: as frases do John em conversa de DOIS turnos.

`medir_john.py` roda uma frase por vez e nao enxerga contexto por construcao — nele
"le esse arquivo" so pode ser `perguntar`, porque nao houve turno anterior. As 265
frases sem alvo ficam medindo abstencao, que e certo, mas deixa sem resposta a
pergunta que interessa: **numa conversa de verdade, quantas delas ela resolve?**

Aqui cada caso e uma conversa:

    turno 1 (armacao)   "le o notas.md"              estabelece o objeto
    turno 2 (a frase)   "Le esse arquivo pra mim"    deve virar ler_arquivo(notas.md)

O turno 1 e escrito por mim e e deliberadamente simples: ele existe so para pousar o
objeto na gaveta certa. Quem esta sendo medido e o turno 2, que e do John.

## O que conta como acerto

Ferramenta certa **e** argumento certo. Resolver "esse arquivo" para o arquivo errado
nao e meio acerto — e a Teka lendo outra coisa.

Para `escrever_arquivo` e `executar_comando` o acerto e OUTRO: elas tem efeito
colateral e o contexto nao as executa por adivinhacao. O esperado ali e a oferta de
confirmacao, e o teste falha se ela agir.

## Por que precisa de mundo real

O contexto so guarda o objeto de uma chamada que **deu certo** — guardar o alvo de
uma leitura que falhou faria "esse arquivo" apontar para um caminho inexistente.
Entao o turno 1 precisa executar de verdade, e por isso `--real` sobre uma pasta
temporaria montada para o teste.

## O CHAO DE RUIDO DESTA REGUA E 19 FRASES DE 265

Medido em 2026-08-28, tres sementes da MESMA configuracao:

    conversa (de 265)    89, 103, 108    media 100,0   desvio 9,9   amplitude 19
    benchmark (de 150)  109, 109, 107    media 108,3   desvio 1,2   amplitude  2

**Oito vezes mais barulhenta que o benchmark.** E faz sentido: cada caso encadeia
intencao no turno 1, execucao, gravacao na gaveta, casamento de anafora, reescrita,
intencao no turno 2 e extracao de argumento. Sete etapas com variancia, compostas.

Consequencia pratica: diferenca menor que ~20 frases aqui **nao diz nada** com uma
semente. Ja aconteceu — uma mudanca no pool de pastas mediu 104 -> 89 e parecia
regressao; com tres sementes a configuracao nova deu 89, 103, 108, e o 104 caiu
dentro da faixa dela.

Para comparar duas versoes: tres sementes de cada lado, no minimo, ou uma sonda
DETERMINISTICA (`--pedido` numa frase especifica), que nao tem semente nem amostragem.

    python medir_conversa.py <modelo.bin> <pasta-do-mundo>
"""
import io
import os
import re
import shutil
import subprocess
import sys
import tempfile

EXE = os.path.join(".", "target", "release", "teka.exe")
FONTE = os.path.join("dados", "frases_john_bruto.txt")

# Armacao por secao: (frase do turno 1, objeto que ela deve pousar na gaveta).
#
# `procurar_arquivo` nao tem armacao de proposito: ele recebe um PEDACO de nome
# ("config"), nao um caminho, e guardar isso como arquivo faria `ler_arquivo` tentar
# abrir "config". A gaveta dele nao existe, entao o esperado continua sendo abster.
ARMACAO = {
    "ler_arquivo": ("le o notas.md", "notas.md"),
    "listar_pasta": ("lista a pasta dados", "dados"),
    "calcular": ("quanto e 340*12", "340*12"),
    "executar_comando": ("roda um tasklist", "tasklist"),
    # Usa a armacao da LEITURA de proposito: `escrever_arquivo` e `ler_arquivo`
    # dividem a gaveta `Arquivo` por desenho, e a extracao de dois argumentos do
    # escrever e fraca nas frases naturais — medido, nenhuma das quatro formas
    # testadas acertou caminho e texto ao mesmo tempo. Armar pela leitura pousa o
    # objeto na gaveta certa sem depender disso.
    "escrever_arquivo": ("le o notas.md", "notas.md"),
    "procurar_arquivo": (None, None),
}
COM_EFEITO = {"escrever_arquivo", "executar_comando"}

CMDS = ["ipconfig", "tasklist", "dir", "hostname", "echo", "ver", "date"]
PASTAS = ["downloads", "documentos", "projetos", "dados", "src", "temp"]


def carregar():
    secoes, atual = {}, None
    for ln in io.open(FONTE, encoding="utf-8"):
        ln = ln.rstrip("\n")
        if ln.startswith("## "):
            atual = ln[3:].strip()
            secoes[atual] = []
        elif ln.strip() and not ln.startswith("#") and atual:
            secoes[atual].append(ln.strip())
    return secoes


def sem_alvo(sec, frase):
    """A frase deixa o objeto implicito? So essas interessam aqui."""
    n = frase.lower()
    if sec == "calcular":
        return not re.search(r"\d", n)
    if sec == "executar_comando":
        return not any(c in n for c in CMDS)
    if re.search(r"[\w-]+\.(txt|json|md|csv|log|rs|toml|ini|py)", n):
        return False
    return not re.search(r"\b(" + "|".join(PASTAS) + r")\b", n)


def conversa(modelo, raiz, turnos):
    """Roda os turnos numa sessao so e devolve a saida do ultimo."""
    entrada = "".join(t + "\n" for t in turnos) + "/sair\n"
    r = subprocess.run(
        # `--sem-memoria`: o modo interativo grava a memoria episodica ao sair, e
        # medir 265 conversas deixava ~500 episodios de artefato no memoria.bin.
        # Nao contaminam o treino (feedback `Nenhum` nao vira exemplo), mas afogam
        # o sinal de percepcao nova do pulso.
        [EXE, "agente", "--carregar", modelo, "--real", raiz, "--sem-memoria"],
        input=entrada, capture_output=True, text=True,
        encoding="utf-8", errors="replace",
    )
    saida = r.stdout or ""
    # Cada turno imprime uma linha comecando por "→". A ultima e a que interessa.
    linhas = [l for l in saida.splitlines() if "→" in l]
    return linhas[-1] if linhas else ""


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return 1
    modelo, raiz = sys.argv[1], sys.argv[2]

    # O modo interativo salva os pesos de volta no .bin carregado e grava memoria
    # episodica. Trabalha numa copia, senao medir altera o que esta sendo medido.
    tmp = os.path.join(tempfile.gettempdir(), "teka_conversa.bin")
    shutil.copy(modelo, tmp)

    secoes = carregar()
    print("modelo: %s   mundo: %s\n" % (modelo, raiz))
    print("%-18s %6s %8s %8s %8s" % ("secao", "casos", "resolveu", "abstem", "errado"))
    print("-" * 54)

    tot = [0, 0, 0, 0]
    for sec, frases in secoes.items():
        if sec not in ARMACAO:
            continue
        arma, objeto = ARMACAO[sec]
        casos = [f for f in frases if sem_alvo(sec, f)]
        if not casos:
            continue
        res = abst = errado = 0
        for f in casos:
            turnos = ([arma] if arma else []) + [f]
            linha = conversa(tmp, raiz, turnos)
            if sec in COM_EFEITO:
                # Certo e OFERECER, nunca agir por adivinhacao.
                if "voce quer dizer" in linha:
                    res += 1
                elif '"acao":"perguntar"' in linha:
                    abst += 1
                else:
                    errado += 1
            else:
                acao = re.search(r'"acao":"([a-z_]+)"', linha)
                nome = acao.group(1) if acao else "?"
                if nome == sec and (objeto is None or objeto in linha):
                    res += 1
                elif nome == "perguntar":
                    abst += 1
                else:
                    errado += 1
        print("%-18s %6d %8d %8d %8d" % (sec, len(casos), res, abst, errado))
        tot[0] += len(casos); tot[1] += res; tot[2] += abst; tot[3] += errado

    print("-" * 54)
    print("%-18s %6d %8d %8d %8d" % ("TOTAL", tot[0], tot[1], tot[2], tot[3]))
    if tot[0]:
        print("\n  resolvidas pelo contexto: %d de %d  (%.1f%%)"
              % (tot[1], tot[0], 100.0 * tot[1] / tot[0]))
        print("  ainda abstem:             %d  (%.1f%%)"
              % (tot[2], 100.0 * tot[2] / tot[0]))
        print("  ferramenta ou alvo errado: %d  (%.1f%%)"
              % (tot[3], 100.0 * tot[3] / tot[0]))
    os.remove(tmp)
    return 0


if __name__ == "__main__":
    sys.exit(main())

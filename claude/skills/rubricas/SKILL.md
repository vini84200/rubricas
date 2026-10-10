---
name: rubricas
description: Cria, lê e altera rubricas e pipelines de checagem automática no servidor Rubricas (rubrica.vini84200.cc) pela API em TOML. Use quando o usuário pedir para criar, montar, ajustar ou revisar uma rubrica, um critério, um roteiro de correção, ou as checagens automáticas (casos de teste, pytest, estilo, plágio) de uma avaliação de EAD.
---

# Rubricas

Cada rubrica é um arquivo TOML no servidor. Leia só a que vai mudar e grave o arquivo inteiro de volta.

```sh
A="Authorization: Bearer $RUBRICAS_TOKEN"
curl -sfH "$A" "$RUBRICAS_URL/api/rubricas.txt"                  # id | título | critérios | passos [| arquivada]
curl -sfH "$A" "$RUBRICAS_URL/api/rubricas/ID.toml"              # o arquivo
curl -sH "$A" -X PUT --data-binary @arq.toml "$RUBRICAS_URL/api/rubricas/ID.toml"   # grava ou cria
```

O `PUT` valida antes de gravar. Ele responde com uma linha, ou com `{"erro": "linha N, coluna M: …"}`. Se faltar `RUBRICAS_URL` ou `RUBRICAS_TOKEN`, peça ao usuário: o token sai de `rubricas token <nome>` no servidor. O ID é o nome do arquivo e vira parte da URL, então use minúsculas e hífens.

## Formato

```toml
titulo = "Prática 5: listas"

[[criterios]]                       # passo de roteiro: orienta quem corrige, não vale nota
tipo = "roteiro"
nome = "Rode com entrada vazia"
descricao = "Execute `python main.py` e aperte Enter. Deve pedir de novo."

[[criterios]]
nome = "Trata entrada vazia"
escala = { tipo = "binaria", pontos = 1.0 }              # atende ou não

[[criterios]]
nome = "Modularização"
descricao = "Aparece no cartão durante a correção."
peso = 2.0                                               # opcional, multiplica os pontos
[criterios.escala]
tipo = "niveis"
[[criterios.escala.niveis]]
rotulo = "Excelente"
descritor = "Funções curtas e bem nomeadas."
pontos = 2.0
[[criterios.escala.niveis]]
rotulo = "Ausente"
pontos = 0.0

[[criterios]]
nome = "Legibilidade"
escala = { tipo = "livre", max = 2.0, passo = 0.25 }     # nota de 0 a max

[[criterios]]
nome = "Estado em structs"
escala = { tipo = "ponto_unico", esperado = "Estado em structs; salva em arquivo.", max = 2.0 }
```

- **A ordem importa:** cada passo de roteiro aparece na correção logo antes do critério que vem depois dele. Intercale cada passo com o critério que ele verifica.
- **Passos de roteiro** não têm `escala` nem `peso`. `descricao` aceita Markdown simples: `código`, blocos ``` ```, listas, **negrito** e *itálico*.
- **Identidade:** critérios e níveis sem `id` são ligados aos já gravados pelo nome. Para **renomear** sem perder as correções feitas, mantenha o id antigo acrescentando `id = "nome-antigo-em-slug"` (minúsculas, sem acento, com hífens) ao item renomeado.
- **Linhas `id =`:** só aparecem quando o id não sai do nome. Preserve as que vierem no arquivo.
- **Critério obrigatório:** a rubrica precisa de pelo menos um critério com escala. Uma escala `niveis` tem pelo menos dois níveis.

## Pipelines (checagens automáticas de EAD)

Uma avaliação pode ter um pipeline: checagens que o executor roda no código de cada aluno e que viram **sugestões** de marcação no cartão (quem decide é o professor). O pipeline pertence à avaliação e usa os ids dos critérios das rubricas dela.

```sh
curl -sfH "$A" "$RUBRICAS_URL/api/avaliacoes.txt"                      # id | título | turma | rubricas | com/sem pipeline [| arquivada]
curl -sfH "$A" "$RUBRICAS_URL/api/pipelines.txt"                        # id | checagens | anexos | pendentes
curl -sfH "$A" "$RUBRICAS_URL/api/pipelines/AV/pipeline.toml"           # o pipeline
curl -sH "$A" -X PUT --data-binary @pipeline.toml "$RUBRICAS_URL/api/pipelines/AV/pipeline.toml"
curl -sH "$A" -X PUT --data-binary @testes.py "$RUBRICAS_URL/api/pipelines/AV/testes.py"   # anexos, também em subpastas
curl -sH "$A" -X DELETE "$RUBRICAS_URL/api/pipelines/AV/referencias/velho.py"
```

Leia a rubrica da avaliação antes (`rubricas/ORIGEM.toml`) para usar os ids certos: o id de um critério é o nome em minúsculas, sem acento, com hífens, ou o `id =` explícito. O `PUT` valida contra a avaliação e avisa os anexos que faltam.

```toml
arquivo = "main.py"         # o arquivo do aluno é copiado com este nome (os testes importam `main`)
tempo = 10                  # segundos por execução; memoria = "256m", imagem = "python:3.12-slim"
plagio = { base = "esqueleto.py", referencias = "referencias/", limiar = 0.6 }   # JPlag; tudo opcional

[[checagens]]
tipo = "casos"              # stdin → stdout; ignora espaços no fim das linhas e os prompts do input()
nome = "Exemplos do enunciado"
criterio = "soma-correta"
sugestao = [{ minimo = 1.0, nivel = "Atende" }, { minimo = 0.5, nivel = "Parcial" }]   # fração de acertos → marcação
[[checagens.casos]]
entrada = "2\n3\n"
saida = "5\n"

[[checagens]]
tipo = "pytest"
testes = "testes.py"        # anexo; importa o arquivo do aluno
criterio = "funcoes"

[[checagens]]
tipo = "estilo"             # ruff check; regras = ["E", "F"], ignorar = ["E501"], formato = true
criterio = "legibilidade"

[[checagens]]
tipo = "comando"            # passa se termina com código 0; $PROGRAMA é o arquivo do aluno
nome = "Compila"
comando = "python3 -m py_compile $PROGRAMA"
```

- **Regras de sugestão:** `nivel` (id ou rótulo) para escala `niveis`, `atende = true/false` para `binaria`, `nota = 1.5` para `livre` e `ponto_unico`. Sem regras: binária só atende com tudo certo; níveis só nos extremos; nota proporcional aos acertos.
- **Casos:** cubra os exemplos do enunciado e as bordas que a especificação cita (entrada vazia, negativo, limite). Os prompts do `input("Digite…")` não entram na saída comparada, porque cada aluno escreve o seu: em `saida`, ponha só o que o programa imprime com `print`. Um prompt feito com `print("Digite:")` antes do `input()` continua na saída: se o enunciado não padroniza isso, prefira comparar com `pytest`. `considerar_prompts = true` (na checagem) os mantém; `ignorar_maiusculas = true` e `argumentos = ["x"]` também são opcionais.
- **Plágio:** `base` é o código dado a todos (não conta como cópia); `referencias` é uma pasta de anexos com soluções conhecidas.
- **Executor:** o servidor nunca roda código. Quem roda é `rubricas executor --servidor URL` (podman), no notebook ou noutra máquina.

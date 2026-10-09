# Plano: roteiro, Claude e correção de EAD

Decisões fechadas em 2026-10-09. Princípio que desempata qualquer escolha: **a UX do avaliador
vem primeiro**. Menos gestos, menos espera, nada que obrigue a sair da tela de avaliar.

Três casos de uso:

1. **Aula prática** (presencial, toda semana): o aluno termina, o professor ou o monitor abre o
   programa dele, segue um roteiro de passos e testes e marca a rubrica no celular.
2. **EAD**: toda semana os alunos enviam um arquivo Python pelo Moodle, a partir de uma
   especificação. O professor corrige com checagens automáticas de apoio.
3. **Questões dissertativas do AMC**: adiado. A única regra é não tomar decisões que o impeçam.

Fases: a **fase 1** fecha o caso 1. A **fase 2** começa só depois de a fase 1 ser usada numa aula
de verdade.

---

## Fase 1: roteiro e Claude

### 1.1 Itens de roteiro na rubrica

Um passo de roteiro é uma entrada na mesma lista `[[criterios]]`, marcada com `tipo = "roteiro"`.
Não tem `escala`, `peso` nem pontos.

```toml
[[criterios]]
tipo = "roteiro"
nome = "Teste com entrada vazia"
descricao = """
Rode `python main.py` e aperte Enter sem digitar nada.
Deve mostrar `Entrada inválida` e pedir de novo.
"""

[[criterios]]
nome = "Trata entrada inválida"
escala = { tipo = "binaria", pontos = 1.0 }
```

- **Por que na mesma lista:** o roteiro vale pela posição, intercalado entre critérios. Uma lista
  separada exigiria apontadores. Uma "escala que vale 0" faria o roteiro parecer critério e
  vazaria para o CSV e para as estatísticas.
- **Nota:** `modelo.rs`, a exportação em `api.rs`, `nota.js` e `util.js` ignoram itens de
  roteiro. Ausente = critério (arquivos antigos continuam válidos).
- **Na tela de avaliar:** cartão próprio, com visual distinto e sem níveis. Qualquer avanço
  (deslizar à direita, `→`, `Enter`) segue em frente. Não conta como "falta marcar" e nada é
  gravado na correção.
- **Quando o fluxo salta** para o próximo critério sem marcação, os passos de roteiro
  imediatamente anteriores aparecem compactos acima do cartão desse critério.
- **Texto:** Markdown simples (código inline, bloco de código com botão de copiar, listas,
  negrito e itálico), com um parser próprio de algumas dezenas de linhas e sem dependência nova.
  Vale também para a `descricao` de critérios.
- **Editor web** (`rubricas.js`): botão "Adicionar passo de roteiro" ao lado de "Adicionar
  critério". A ordem dos dois tipos juntos muda com setas ↑/↓ (funciona bem no celular).

### 1.2 Tokens de API

- `rubricas token NOME [--remover]` cria ou remove um token. O token aparece uma vez só, e o
  hash fica em `usuarios.toml`.
- Enviado como `Authorization: Bearer …`, com os mesmos poderes do usuário. Remover o usuário ou
  trocar a senha invalida os tokens dele.
- Na máquina local, fica em `RUBRICAS_TOKEN`, nunca no repositório.

### 1.3 Endpoints em texto, para gastar poucos tokens

| Pedido | Resposta |
| --- | --- |
| `GET /api/rubricas.txt` | uma linha por rubrica: `id \| título \| nº critérios \| nº passos` |
| `GET /api/rubricas/{id}.toml` | o TOML, sem os `id` que coincidem com o derivado do nome |
| `PUT /api/rubricas/{id}.toml` | valida e grava. Erro: mensagem curta com linha e coluna |

No `PUT`, critérios e níveis sem `id` são **associados aos existentes pelo nome**. Assim, o
Claude reescrever o arquivo não muda identidades nem quebra correções já feitas.

Depois dá para pôr um `/mcp` fino por cima destes mesmos endpoints, para usar pelo claude.ai ou
pelo celular. Isso fica fora da fase 1.

### 1.4 Skill do Claude Code

- O código-fonte fica em `claude/skills/rubricas/SKILL.md`. `rubricas skill instalar` copia para
  `~/.claude/skills/rubricas/`, para que funcione com o Claude Code aberto em qualquer pasta (por
  exemplo, a pasta da disciplina, com as especificações).
- Conteúdo: o formato TOML em poucas linhas, com exemplo de `roteiro`, mais os comandos `curl`
  para listar, ler e gravar, usando `RUBRICAS_URL` e `RUBRICAS_TOKEN`.
- Instrução central: ler só a rubrica que vai mudar e devolver o arquivo inteiro num `PUT`.

### 1.5 Publicação

- `Dockerfile` multi-stage com imagem final mínima, e `compose.yaml` de exemplo com volume para
  `dados/` e `--endereco 0.0.0.0:8080`.
- Produção: `rubrica.vini84200.cc`, em Docker atrás do proxy do Cloudflare (HTTPS).

---

## Fase 2: EAD

### 2.1 Submissões do Moodle

- Você envia o ZIP de "Baixar todas as submissões" da tarefa na tela da avaliação. O servidor
  reconhece as pastas `Nome Sobrenome_12345_assignsubmission_file_/` e associa cada uma a um
  aluno da turma (importada do CSV de participantes do Moodle).
- Ficam guardadas em `dados/submissoes/<avaliacao>/<aluno>/`.
- **Reenvio:** a submissão de quem está no ZIP é substituída e a dos ausentes é mantida. Se uma
  submissão substituída já tinha correção, ela ganha o aviso "submissão mudou depois da correção",
  sem apagar as marcações.
- **Nomes que não casam** vão para uma lista "sem dono", para associação manual. A associação é
  lembrada nos próximos ZIPs.
- A API de Web Services do Moodle (token da TI) fica como melhoria futura.

### 2.2 Visualizador de código

Python com números de linha e destaque de sintaxe (Prism core + Python em `web/vendor/`, sem
etapa de build). Em tela larga, aparece ao lado do cartão na tela de avaliar. Sem comentários por
linha: o comentário continua sendo por critério.

### 2.3 Tela "Acompanhar" (monitor + celular)

Opcional. Aberta no monitor, mostra o código e os relatórios do aluno que **o mesmo usuário**
está avaliando em outro aparelho e troca sozinha quando ele troca de aluno. Funciona por SSE, sem
pareamento: basta estar logado nos dois. Um *heartbeat* a cada ~30 s evita que o Cloudflare corte
a conexão (ele corta conexões ociosas em ~100 s).

### 2.4 Pipelines de checagem

O servidor **nunca executa código de aluno** (é público).

**Definição:** `dados/pipelines/<id>/pipeline.toml`, atrelado à avaliação, com os anexos na mesma
pasta (`testes.py`, `esqueleto.py`, `referencias/`). Leitura e gravação são pela API em texto, e a
mesma skill do Claude cobre pipelines. A web só mostra o pipeline e os resultados. Tipos de
checagem:

| Tipo | O que faz |
| --- | --- |
| `casos` | stdin → stdout esperado. Ignora espaço no fim e, opcionalmente, maiúsculas e minúsculas |
| `pytest` | arquivo de testes do professor que importa o arquivo do aluno |
| `estilo` | `ruff check` e, opcionalmente, `ruff format --check`, com regras configuráveis |
| `plagio` | JPlag (ver 2.5) |
| `comando` | qualquer comando dentro do contêiner |

**Sugestões, nunca marcações:** uma checagem pode declarar `criterio = "<id>"` com regras do tipo
"todos passaram → nível máximo; ≥50% → nível X". O cartão mostra "Sugestão: Atende (8/8 casos)" e
uma tecla (`G`) aceita. Quem decide é sempre o avaliador.

**Erros:** código do aluno que não roda (sintaxe, exceção, tempo esgotado) conta como reprovado,
e o relatório mostra o fim do stderr. Falha do próprio executor (imagem ausente, podman fora do
ar) aparece como "erro de execução", não reprova e continua pendente.

### 2.5 Executor

- `rubricas executor --servidor URL [--continuo]` autentica com token. Busca o que está pendente
  (submissão nova ou alterada, ou pipeline editado), roda e envia os resultados. Sem
  `--continuo`, termina ao esvaziar a fila. Com ele, fica ouvindo.
- Pode rodar no notebook ou na mesma VPS. A tela da avaliação mostra "N submissões aguardando o
  executor".
- Cada checagem roda em `podman run --network=none`. Padrões configuráveis no pipeline:
  `python:3.12-slim`, 10 s, 256 MB, 1 CPU. `ruff` e `pytest` vão numa imagem derivada construída
  uma vez.
- **Plágio:** JPlag (Java, GPL-3.0), rodado como processo separado num contêiner, sem ser ligado
  ao nosso código. Só compara submissões da mesma avaliação, mais um código-base opcional
  (esqueleto fornecido, que não conta como plágio) e referências opcionais (a solução do
  professor, versões da internet). Exemplo:
  `plagio = { base = "esqueleto.py", referencias = "referencias/", limiar = 0.6 }`.
  Confirmar as opções exatas de linha de comando do JPlag na documentação ao implementar.
- **Resultado do plágio:** o servidor recebe só os pares e as similaridades. A tela mostra os
  pares mais parecidos e um botão que baixa o `.jplag` para o visualizador oficial. No cartão de
  avaliar, o selo "⚠ 87% parecido com Fulano" aparece só acima do limiar.

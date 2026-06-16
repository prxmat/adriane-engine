# Adriane CLI (`adriane`)

> **État pre-release.** La CLI s'exécute **depuis les sources** du monorepo. Le paquet `@adriane/adriane-cli` est `private` et n'est pas publié sur npm. Les commandes ci-dessous sont décrites telles qu'implémentées dans `packages/adriane-cli/src/`.

La CLI `adriane` est l'outil en ligne de commande pour **valider**, **compiler**, **exécuter**, **publier**, **comparer** et **initialiser** des définitions Adriane (graphes, agents, prompts). Elle est construite sur `commander` (`packages/adriane-cli/src/cli.ts`).

```
adriane <commande> [arguments] [options]
```

Le binaire est déclaré comme `adriane` (`bin` → `./bin/adriane.js`) et produit par le build (`tsc` + `scripts/make-bin.mjs`).

> **Détection du type de fichier.** Pour `validate` et `compile`, un fichier dont le nom contient `.graph.` est traité comme un **graphe** (compilé via `graph-adriane`) ; sinon, il est traité comme un fichier **prompt / agent / chain** (compilé via `lang-adriane`, qui détecte le sous-type). `run` et `diff` opèrent exclusivement sur des fichiers de **graphe**.

---

## `validate <file>`

Valide un fichier Adriane et affiche les diagnostics.

```bash
adriane validate ./my-flow.graph.yaml
adriane validate ./my-agent.yaml
```

- Compile le fichier (graphe ou prompt/agent/chain selon le nom) et formate les diagnostics.
- N'écrit rien sur le disque.
- **Code de sortie** : `1` si au moins un diagnostic de sévérité `error`, sinon `0`.

---

## `compile <file> --out <dir>`

Compile un fichier en sa forme JSON et l'écrit sur disque.

```bash
adriane compile ./my-flow.graph.yaml --out ./dist
```

| Option | Requis | Description |
| --- | --- | --- |
| `--out <dir>` | oui | Dossier de sortie (créé récursivement si absent) |

- Si la compilation produit une erreur (ou aucun résultat), affiche les diagnostics et sort avec le code `1` — **rien n'est écrit**.
- Sinon, écrit `<dir>/<nom-de-base-sans-extension>.json` (JSON indenté à 2 espaces) et affiche `Wrote <fichier>`.
- **Code de sortie** : `1` en cas d'erreur de compilation, sinon `0`.

---

## `run <file> [--input <json>] [--watch]`

Exécute un fichier de **graphe** et diffuse les événements d'exécution en mode `debug` sur la sortie standard (un objet JSON par ligne).

```bash
adriane run ./my-flow.graph.yaml
adriane run ./my-flow.graph.yaml --input '{"name":"Ada"}'
adriane run ./my-flow.graph.yaml --watch
```

| Option | Requis | Description |
| --- | --- | --- |
| `--input <json>` | non | Données initiales du run, en JSON. Défaut : `{}` |
| `--watch` | non | Ré-exécute à chaque modification du fichier |

Comportement (`packages/adriane-cli/src/commands/run.ts`) :

- Compile le graphe via `graph-adriane`. En cas d'erreur de compilation, écrit `<code>: <message>` sur **stderr** et n'exécute pas.
- Construit un runtime en mémoire (`InMemoryNodeRegistry`, `InMemoryConditionRegistry`, `InMemoryCheckpointer`, `InMemoryEventBus`). **Chaque nœud est enregistré avec un handler no-op** (`async () => ({})`) : `run` valide et trace le flux d'exécution du graphe — il n'exécute pas de logique métier de nœud.
- Diffuse via `runtime.stream(..., "debug")` ; chaque événement est écrit en JSON ligne par ligne sur stdout.
- **Code de sortie** : `0`.

> En mode `--watch`, la CLI réutilise les mêmes données `--input` à chaque ré-exécution déclenchée par un changement de fichier.

---

## `publish <file> --registry <url>`

Publie le **contenu brut** d'un fichier vers une registry via HTTP `POST`.

```bash
adriane publish ./my-flow.graph.yaml --registry https://registry.example.com/graphs
```

| Option | Requis | Description |
| --- | --- | --- |
| `--registry <url>` | oui | URL de la registry cible |

Comportement (`packages/adriane-cli/src/commands/publish.ts`) :

- Lit le fichier et envoie son contenu en `POST` avec l'en-tête `content-type: application/yaml`.
- Si la réponse n'est pas `ok`, écrit `Publish failed: <status>` sur stderr et sort avec le code `1`.
- Sinon, écrit `Published successfully.`.
- **Code de sortie** : `1` si la requête échoue, sinon `0`.

---

## `diff <left> <right>`

Compare deux fichiers de **graphe** et affiche les nœuds, arêtes et canaux ajoutés/retirés. Outil de **diagnostic**.

```bash
adriane diff ./v1.graph.yaml ./v2.graph.yaml
adriane diff ./flow.graph.yaml@1.0.0 ./flow.graph.yaml@2.0.0
```

- Chaque argument accepte la forme `<fichier>@<version>` ; la partie après `@` est utilisée uniquement comme **libellé** dans l'en-tête de sortie (`Diff <left> -> <right>`), pas pour résoudre une version.
- Compile les deux graphes via `graph-adriane`. Si l'un est invalide, écrit `Unable to diff invalid graph files.` sur stderr et sort avec le code `1`.
- Sinon, affiche les ensembles ajoutés (`+`) et retirés (`-`) pour les **nœuds**, **arêtes** et **canaux** (`-` quand vide).
- **Code de sortie** : `1` si un graphe est invalide, sinon `0`.

---

## `init <kind> --id <id> --out <file>`

Crée un fichier modèle pour un graphe, un agent ou un prompt.

```bash
adriane init graph  --id my-flow   --out ./my-flow.graph.yaml
adriane init agent  --id my-agent  --out ./my-agent.yaml
adriane init prompt --id my-prompt --out ./my-prompt.yaml
```

| Argument / Option | Requis | Description |
| --- | --- | --- |
| `<kind>` | oui | `graph` \| `agent` \| `prompt` |
| `--id <id>` | oui | Identifiant injecté dans le modèle |
| `--out <file>` | oui | Chemin de sortie (dossier parent créé si absent) |

Modèles générés (`packages/adriane-cli/src/commands/init.ts`) :

- **graph** — `id`, `version: 1.0.0`, `name`, `entryNodeId: start`, un canal `state` (`type: object`, `reducer: merge`, `default: {}`), un nœud `start` de type `action`, `edges: []`.
- **agent** — `id`, `description`, `prompt: <id>.prompt`, `tools: []`.
- **prompt** — `name`, `template: "Hello {{name}}"`, `variables: [name]`.

Écrit le fichier et affiche `Initialized <kind> template at <fichier>`.

**Code de sortie** : `0`.

---

## Récapitulatif des codes de sortie

| Commande | `0` | `1` |
| --- | --- | --- |
| `validate` | aucun diagnostic d'erreur | au moins un diagnostic `error` |
| `compile` | écrit le JSON | erreur de compilation (rien écrit) |
| `run` | toujours | — (erreurs de compilation tracées sur stderr) |
| `publish` | réponse HTTP `ok` | requête échouée |
| `diff` | les deux graphes valides | un graphe invalide |
| `init` | toujours | — |

## Voir aussi

- [`architecture.md`](./architecture.md) — le pipeline DSL (`parser → ast → validator → transformer → compiler`), le runtime et le pont napi.

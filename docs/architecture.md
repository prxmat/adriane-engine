# Adriane Engine — Architecture

> **État pre-release.** Le moteur s'installe **depuis les sources** (monorepo pnpm + crates Rust). Aucun paquet npm/crate publié n'est requis. La compilation de l'addon natif est optionnelle (voir [Pont napi Rust ↔ TS](#pont-napi-rust--ts)).

Adriane est un **runtime de graphes d'agents** : exécution **déterministe**, **resumable** (reprise depuis le dernier checkpoint) et **observable** (un événement par transition de cycle de vie de nœud). Le moteur est écrit en Rust (chemin de production) avec un **SDK TypeScript** comme porte d'entrée publique (`@adriane-ai/graph-sdk`). Une implémentation TypeScript équivalente du runtime sert de **fallback** quand l'addon natif est absent.

---

## Les couches du moteur

Le moteur respecte une règle de dépendance **unidirectionnelle** : `graph-core` (zéro dépendance) → `graph-runtime` → `graph-sdk` (API publique). `graph-core` ne dépend d'aucun autre paquet interne ; toute importation inverse violerait le contrat de fondation.

```
graph-core   (modèle pur, zéro effet de bord, zéro dépendance interne)
    │
graph-runtime   (moteur d'exécution : NodeRegistry, ConditionRegistry, Checkpointer, EventBus)
    │
graph-sdk   (API publique fluide : createGraph → CompiledGraph)
```

### `graph-core` — le modèle de données pur

TypeScript pur, **aucun I/O** (pas de DB, HTTP, LLM, framework). Définit les types fondateurs, tous *branded* (`NodeId`, `EdgeId`, `GraphId`, `RunId`), ainsi que les schémas et erreurs.

Types clés (`packages/graph-core/src/types.ts`) :

| Type | Forme |
| --- | --- |
| `GraphDefinition` | `{ id, version, name, recursionLimit?, channels, nodes[], edges[], entryNodeId, metadata? }` |
| `GraphState` | `{ runId, graphId, currentNodeId, status, channels, version, checkpointId?, createdAt, updatedAt }` |
| `NodeDefinition` | `{ id, type, label, subgraphId?, inputMapping?, outputMapping?, fanOut?, retryPolicy?, metadata? }` |
| `EdgeDefinition` | `{ id, from, to, type, condition? }` |
| `Command` | `{ goto: NodeId \| NodeId[], update? }` |
| `RetryPolicy` | `{ maxAttempts, backoffMs }` |

Énumérations :

- **Types de nœud** (`NODE_TYPES`) : `"action"`, `"agent"`, `"tool"`, `"human-gate"`, `"subgraph"`.
- **Types d'arête** (`EDGE_TYPES`) : `"default"` (toujours suivie), `"conditional"` (résolue depuis le `ConditionRegistry` par nom).
- **Statuts de run** (`GRAPH_STATUSES`) : `"idle"`, `"running"`, `"suspended"`, `"completed"`, `"failed"`.
- **Réducteurs de canal** (`ChannelReducer`) : `"replace"` (écrase), `"append"` (ajoute à une liste), `"merge"` (fusionne un objet).

> **Canaux et réducteurs.** Un canal `messages` typé est `{ type: "messages", reducer: "append", default: [] }` ; l'ordre des messages est préservé. Les canaux `replace` écrasent la valeur précédente.

### `graph-runtime` — le moteur d'exécution

Construit au-dessus d'une `GraphDefinition` validée. Quatre abstractions cœur (`packages/graph-runtime/src/interfaces.ts`) :

| Abstraction | Rôle | Implémentation par défaut |
| --- | --- | --- |
| `NodeRegistry` | `register(nodeId, handler)` / `resolve(nodeId)` — associe un nœud à son handler | `InMemoryNodeRegistry` |
| `ConditionRegistry` | `register(name, fn)` / `resolve(name)` — associe un **nom** de condition à un prédicat pur | `InMemoryConditionRegistry` |
| `Checkpointer` | `save` / `load(runId)` / `loadById(id)` / `list(runId)` | `InMemoryCheckpointer` |
| `EventBus` | `emit(event)` / `subscribe(handler) → unsubscribe` | `InMemoryEventBus` |

Signature d'un handler de nœud :

```ts
type NodeHandler = (
  input: TInput,
  state: GraphState,
  context: NodeExecutionContext   // { memory: BaseStore }
) => Promise<Partial<ResolvedChannels> | Command>;
```

Un handler retourne soit une **mise à jour de canaux** (`Partial<ResolvedChannels>`), soit un `Command { goto, update? }` qui prend le pas sur la résolution d'arête par défaut.

Un prédicat de condition est **synchrone et pur** :

```ts
type ConditionFn = (state: GraphState) => boolean;
```

> **Invariant de sécurité.** Les conditions sont **toujours des noms** résolus dans le `ConditionRegistry` — jamais du code `eval`'é ni des expressions JS inline. C'est non négociable (inspection + sûreté).

### `graph-sdk` — l'API publique

`createGraph(options)` renvoie un `GraphBuilder<TState>` fluide qui accumule canaux, nœuds, arêtes, handlers et conditions, puis `.compile()` produit un `CompiledGraph<TState>` avec inférence de type complète. Voir [`cli.md`](./cli.md) et les exemples auto-vérifiants sous `packages/graph-sdk/examples/`.

Méthodes principales du builder (`packages/graph-sdk/src/builder.ts`) :

- `channel(name, { type, reducer?, default? })` — déclare un canal typé (`reducer` défaut `"replace"`).
- `messagesChannel(name?)` — canal `messages` (`append`, défaut `[]`).
- `node(id, handler | config)` — nœud `action` (défaut) ou config.
- `humanGate(id, { label? })` — nœud `human-gate` qui suspend le run.
- `agentNode(id, config)` — agent ReAct ; son `AgentResult` atterrit dans `config.outputChannel` (défaut `"agentResult"`, auto-déclaré).
- `compile()` → `CompiledGraph`.

Le premier nœud ajouté devient le point d'entrée par défaut.

Méthodes du graphe compilé (`packages/graph-sdk/src/compiled-graph.ts`) :

- `run(initialData?, { runId? })` — démarre un run frais jusqu'à complétion ou suspension.
- `resume(runId)` — reprend depuis le dernier checkpoint.
- `approveAndResume(runId, { approvedTools })` — accorde l'approbation d'outils et reprend.
- `stream(initialData, mode, options?)` — flux d'événements (voir [Streaming](#streaming)).
- `onEvent(handler)` — souscrit au flux de cycle de vie ; renvoie une fonction de désinscription.
- `usesRustEngine` — `true` quand l'exécution passe par le moteur Rust.

---

## Contrat du runtime

Le contrat est **déterministe par défaut**, **reprenable** et **observable**. Quatre garanties, vérifiées dans le runtime TS (`packages/graph-runtime/src/runtime.ts`) et répliquées côté Rust :

### 1. Checkpoint après chaque nœud

Un checkpoint est persisté **après chaque exécution de nœud** (pas seulement sur mutation d'état). C'est le fondement de la reprenabilité : toute interruption renvoie au dernier checkpoint.

- `start(runId, initialData)` construit l'état initial (`status: "running"`, entrée = `entryNodeId`), **persiste immédiatement un checkpoint**, puis lance la boucle.
- Chaque transition de nœud appelle `persistCheckpoint(state)` puis émet sur l'`EventBus`.

Structure d'un checkpoint (`packages/graph-runtime/src/types.ts`) :

```ts
type Checkpoint = { id: CheckpointId; runId: RunId; graphState: GraphState; createdAt: string };
```

### 2. Un événement par transition de cycle de vie

L'union `RunEvent` (`packages/graph-runtime/src/types.ts`) :

| Événement | Émis quand |
| --- | --- |
| `node_started` | un nœud commence |
| `node_completed` | un nœud termine (porte `output`) |
| `node_failed` | un nœud échoue (porte `error`, `attempt`) |
| `run_suspended` | le statut passe à `suspended` (human-gate ou interrupt dynamique ; porte `reason`) |
| `run_resumed` | une reprise repart d'un état suspendu |
| `run_completed` | le run atteint un état terminal (porte `finalState`) |
| `run_failed` | le run échoue (porte `error`) |

Les événements sont synchrones sur l'`InMemoryEventBus` (fire-and-forget par design).

### 3. Suspend / resume déterministe

Les nœuds `human-gate` **suspendent proprement** : à l'exécution, `executeNode` détecte le type `human-gate`, appelle `suspendRun(...)` (statut `suspended`, raison `"human-gate"`) et émet `run_suspended`. Les **interrupts dynamiques** (`DynamicInterrupt`) suspendent un run depuis n'importe quel nœud avec une `reason`.

À la reprise (`resume(runId)`) :

1. Charge le **dernier checkpoint** (`checkpointer.load`). Sans checkpoint, c'est une erreur.
2. Si l'état est `suspended` **et** que le nœud courant est un `human-gate` (ou que l'interrupt s'applique `"after"`), avance vers le nœud suivant ; sinon, reste sur le nœud courant.
3. Émet `run_resumed`, persiste un checkpoint, relance la boucle.

### 4. Limite de récursion

`GraphDefinition.recursionLimit` (optionnel) borne le nombre de pas par run pour empêcher les cycles infinis. Le dépassement lève `RecursionLimitError` **pendant** le run (pas à la validation). En l'absence de limite, le défaut est `Number.MAX_SAFE_INTEGER`.

### Robustesse à la reprise

- À la relecture d'un checkpoint persisté, l'état est **re-validé** par Zod (`parseGraphState`) — garde contre un état corrompu ou altéré.
- L'égalité d'état utilise `structuralEqual` (cycle-safe, insensible à l'ordre des clés), pas `JSON.stringify`.

> **Réservé, non implémenté.** Le fan-out parallèle (`NodeDefinition.fanOut`) et les sous-graphes (`NodeDefinition.subgraphId`) ont des emplacements dans le schéma. Le fan-out parallèle n'est pas implémenté dans le runtime. Ne pas s'appuyer dessus tant qu'ils ne sont pas marqués stables.

---

## NodeRegistry / ConditionRegistry / Checkpointer / EventBus

Ce sont les **points d'extension** du runtime. Les implémentations `InMemory*` sont les défauts et sont **en mémoire uniquement** :

- `InMemoryNodeRegistry` — `register(nodeId, handler)`, `resolve(nodeId)`.
- `InMemoryConditionRegistry` — `register(name, fn)`, `resolve(name)`. Les prédicats sont des fonctions pures nommées.
- `InMemoryCheckpointer` — `save`, `load(runId)` (dernier checkpoint), `loadById(id)`, `list(runId)`.
- `InMemoryEventBus` — `emit`, `subscribe` (renvoie la fonction de désinscription).

> Pour la production, une persistance durable des checkpoints (ex. Postgres) passe par une implémentation `Checkpointer` personnalisée fournie au runtime — elle n'est pas exportée par le SDK public.

### Streaming

`CompiledGraph.stream(initialData, mode, options?)` expose plusieurs modes (côté moteur TS, `packages/graph-runtime/src/stream.ts`) :

| Mode | Émission |
| --- | --- |
| `"values"` | l'état complet après chaque nœud (`state_value`) |
| `"updates"` | le delta de canaux par nœud (`state_update`) |
| `"messages"` | les événements `Message` extraits |
| `"debug"` | les stages `node_started` / `node_completed` / `state` / `checkpoint` |

> Sur le **moteur Rust**, il n'existe pas (encore) de surface de stream incrémental : `stream()` pilote un run complet et émet un unique `state_value` terminal. Sur le **moteur TS**, le stream est natif.

---

## Pont napi Rust ↔ TS

Le moteur Rust est exposé à TypeScript via un **addon natif** (napi-rs). Le SDK le charge dans un `try/catch` ; s'il est absent, il **retombe** sur le moteur TS.

### Chargement de l'addon (`crates/bindings/index.js`)

Ordre de résolution (premier trouvé l'emporte) :

1. `./adriane_napi.node` — build local de dev (`scripts/build-napi.sh`).
2. `./adriane_napi.<triple>.node` — build local par plateforme.
3. `@adriane-ai/napi-<triple>` — paquet prebuilt par plateforme.

Cibles couvertes : **darwin** (arm64/x64), **linux glibc** (x64/arm64), **win32 x64**. Non livrées : linux musl/Alpine, win32 arm64, autres arches — sur celles-ci le module **throw**, et le SDK retombe sur le moteur TS.

> **Construire l'addon localement** : `bash scripts/build-napi.sh` (ou `pnpm napi:build`). Sans lui, `usesRustEngine` vaut `false` et le SDK utilise silencieusement le moteur TS.

### Surface napi (`crates/bindings/index.d.ts`)

Fonctions **synchrones** :

- `engineVersion(): string` — version du moteur Rust lié.
- `validateGraphJson(definitionJson): string` — renvoie un tableau JSON d'erreurs de validation (`[]` si valide).
- `compileGraphYamlJson(yaml): string` — compile le DSL graphe YAML en `GraphDefinition` (JSON).

Fonctions **asynchrones** (renvoient une `Promise<string>` = `RunOutcome` JSON) :

- `engineRun(specJson, onNode, onCondition, onEvent)` — démarre un run frais.
- `engineResume(specJson, onNode, onCondition, onEvent)` — reprend depuis l'état sérialisé (`specJson.state`).
- `engineApproveAndResume(specJson, onNode, onCondition, onEvent)` — accorde `specJson.approvedTools` (écrits dans le canal `__approvedTools`) et reprend.

### Les trois callbacks JS (seams)

Côté Rust, ce sont des `ThreadsafeFunction` (TSFN) que le moteur attend, permettant une exécution asynchrone aller-retour à travers la frontière de langage **sans bloquer le thread principal JS** (`crates/bindings/src/bridge.rs`) :

| Callback | Signature | Comportement |
| --- | --- | --- |
| `onNode(payloadJson)` | `string \| Promise<string>` | Handler de nœud JS (`kind:"node"` → JSON de mise à jour de canaux) ou `execute` d'outil JS (`kind:"tool"` → JSON de résultat). **Awaité** par Rust. |
| `onCondition(payloadJson)` | `boolean \| string \| Promise<…>` | Prédicat nommé (`{ name, state }`) ; renvoie un booléen ou `"true"`/`"false"`. **Awaité** par Rust. |
| `onEvent(payloadJson)` | `void` | Puits d'événements de cycle de vie (`RunEvent` sérialisé). **Fire-and-forget**, jamais awaité. |

### Le contrat de fil `EngineSpec` (`crates/bindings/src/spec.rs`)

Le SDK envoie un `EngineSpec` en **JSON camelCase** qui doit correspondre exactement aux types `@adriane-ai/graph-core` :

```
EngineSpec {
  graph,            // GraphDefinition
  runId?,
  initialData,      // map<string, value>
  state?,           // GraphState sérialisé (requis par resume/approve)
  approvedTools[],  // écrits dans __approvedTools (chemin approve)
  agents,           // map<nodeId, AgentSpec>
  componentNodes,   // map<nodeId, ComponentNodeSpec> — composants Rust natifs
  jsNodeIds[],      // nœuds dont le handler est une closure JS
  jsToolNames[]     // outils dont l'execute est une closure JS
}
```

`AgentSpec` (par nœud agent) : `{ provider, model?, tier?, system?, toolNames[], maxIterations?, suspendForApproval, approvalToolNames[], outputChannel? }`. Le `tier` (`"frontier" | "balanced" | "fast" | "creative"`) est résolu côté Rust contre les providers disponibles dans l'environnement ; un `model` explicite l'emporte toujours.

`ComponentNodeSpec` (par nœud composant) : `{ kind, params }` — exécuté par un handler Rust natif (et **non** routé vers le seam JS), même si l'id apparaît aussi dans `jsNodeIds`.

Le pont (`bridge.rs`) désérialise l'`EngineSpec`, construit le `GraphRuntime` Rust, câble les TSFN comme seams, pilote `start`/`resume`/`approve`, puis re-sérialise un `RunOutcome` (`{ state, status, pendingApprovals }`).

> **Round-trip de checkpoint.** La sérialisation camelCase doit correspondre exactement de part et d'autre. Une divergence casse les reprises depuis checkpoint.

### Canaux réservés (chemin d'approbation)

- `__approvedTools` — noms d'outils approuvés par un humain, écrits avant reprise.
- `__approvalIds` — ids de requêtes d'approbation par outil (chemin `ApprovalEngine`, côté TS).

> **Un agent n'approuve jamais ses propres outils.** L'approbation est un seam humain : `suspendForApproval` suspend le run, puis `approveAndResume(runId, { approvedTools })` débloque.

---

## Pipeline DSL : parser → ast → validator → transformer → compiler

Deux compilateurs DSL suivent le **même pipeline** :

- `lang-adriane` — DSL de **prompt / agent / chain** (`packages/lang-adriane/src/compiler/compile-file.ts`).
- `graph-adriane` — DSL de **graphe** → `GraphDefinition` (`packages/graph-adriane/src/compiler/compile-graph-file.ts`).

### `graph-adriane` (graphe YAML → `GraphDefinition`)

```
YAML
  → yaml.load          (parse brut)
  → buildGraphAST      (parser/  → AST)
  → validateGraphAST   (validator/ → Diagnostic[])    ── si une erreur : on s'arrête, result = undefined
  → transformGraph     (transformer/ → GraphDefinition)
```

`compileGraphFile(content, file)` renvoie `{ result?: GraphDefinition; diagnostics: Diagnostic[] }`. Si un diagnostic a la sévérité `"error"`, `result` est `undefined`.

### `lang-adriane` (prompt / agent / chain)

```
YAML
  → parseYaml          (parser/)
  → detectKind         ("prompt" | "agent" | "chain") — via _kind, ou la présence de `template` / `steps`
  → build{Prompt|Agent|Chain}AST       (parser/)
  → validate{Prompt|Agent|Chain}AST    (validator/ → Diagnostic[])
  → transform{Prompt|Agent|Chain}       (transformer/)
```

`compileFile(content, file)` renvoie `{ result?: PromptTemplate | AgentConfig | ChainDefinition; diagnostics: Diagnostic[] }`.

> Le pipeline DSL est aussi disponible nativement côté Rust via `compileGraphYamlJson` (napi). Le SDK et la CLI utilisent ce chemin lorsque l'addon est présent.

---

## Voir aussi

- [`cli.md`](./cli.md) — la CLI `adriane` (validate / compile / run / publish / diff / init).
- `packages/graph-sdk/examples/` — exemples auto-vérifiants (chaque assertion `exit(1)` à l'échec, servant de smoke tests).
- `.cursor/rules/*.mdc` — règles par couche (faisant autorité).

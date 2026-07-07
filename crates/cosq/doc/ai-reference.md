# cosq — AI Reference Documentation

## Tool Description

cosq is a read-only CLI for querying Azure Cosmos DB instances. It connects via
Azure CLI authentication (tokens cached on disk) and supports ad-hoc SQL,
natural-language questions (`ask`), semantic/full-text search (Cosmos-native),
stored queries with parameters, multi-step pipelines, an interactive shell,
a query-cost doctor (`explain`), cached AI schema cards, multi-account
profiles, and multiple output formats. cosq never writes to Cosmos DB.

## Complete CLI Command Reference

### `cosq query <SQL>`

Execute an ad-hoc SQL query against Cosmos DB.

| Flag          | Description                         |
|---------------|-------------------------------------|
| `--db`        | Database name (overrides config)    |
| `--container` | Container name (overrides config)   |
| `-o, --output`| Output format: json, json-compact, table, csv, template |
| `--template`  | Path to a MiniJinja template file   |
| `--pk`        | Scope to one partition key value (skips fan-out; much cheaper) |
| `--first`     | Stop after N documents              |
| `--max-items` | Page size per request               |

Queries whose WHERE clause pins the container's partition key to a single
value are automatically scoped to that partition (look for "Scoped to
partition" on stderr). Request charge (RU) is printed to stderr; results go
to stdout, so piping is clean.

Example:

```bash
cosq query "SELECT c.id, c.name FROM c WHERE c.type = 'user'" --db mydb --container users
```

### `cosq ask "<QUESTION>"`

Ask a natural-language question; the AI generates Cosmos SQL grounded in the
container's schema card, executes it, and prints the results. The generated
SQL and a one-line explanation go to stderr; results go to stdout.

| Flag          | Description                              |
|---------------|------------------------------------------|
| `--db` / `--container` | Target (overrides config)       |
| `-o, --output`| Output format                            |
| `--save NAME` | Save the generated SQL as a stored query |
| `--sql-only`  | Print the generated SQL without executing|
| `-y, --yes`   | Skip low-confidence confirmation prompts |

```bash
cosq ask "how many orders were cancelled last week, by region?" -y -o json
cosq ask "top 5 customers by total order value" --save top-customers
```

### `cosq search "<TEXT>"`

Semantic / full-text / hybrid search using Cosmos DB's own search engine
(no local index). Mode auto-detected from container policies: vector policy →
query text is embedded via ailloy and matched with `VectorDistance`; full-text
policy → BM25 `FullTextScore`; both → RRF hybrid; neither → keyword CONTAINS
fallback. Vector results include a `_score` field; raw embeddings and Cosmos
system fields are stripped.

| Flag          | Description                                   |
|---------------|-----------------------------------------------|
| `--mode`      | Force `vector`, `text`, or `hybrid`           |
| `--top N`     | Number of results (default 10)                |
| `--show-sql`  | Print the search SQL without executing        |
| `--pk VALUE`  | Scope to one partition (exact ranking)        |

Requires an embed-capable ailloy node whose model matches the container's
stored vectors (matched by dimensions, remembered per container).

### `cosq explain "<SQL>"`

The query doctor: re-runs the query with query metrics + index metrics and
prints cost (RU), timings, documents retrieved vs returned, which indexes
were used, recommended single/composite indexes (with the indexingPolicy
JSON), and — when AI is enabled — a plain-language diagnosis. Read-only:
prints fixes, never applies them.

### `cosq schema [CONTAINER]`

Show (building if needed) the container's schema card: field paths, types,
example values, low-cardinality value sets, AI descriptions, inferred
cross-container relationships, partition key, and vector/full-text policies.
Cards are cached at `~/.cosq/schema/<profile>/<db>/<container>.yaml` for 7
days (`COSQ_SCHEMA_TTL_DAYS`); a project-local `.cosq/schema/` copy wins.
`--refresh` rebuilds; `--json` for machine-readable output.

### `cosq shell`

Interactive REPL holding context (profile, db, container, format). Type SQL
directly (multi-line; `;` terminates), `? question` for ask-mode with
conversation memory (follow-ups compose), and `:` meta-commands (`:db`,
`:container`, `:profile`, `:format`, `:queries`, `:run`, `:schema`,
`:search <text>`, `:explain`, `:help`, `:quit`). Tab completion covers
meta-commands, database/container names, and stored-query names. Piped stdin
runs the same dispatch non-interactively (scriptable).

### `cosq databases` / `cosq containers [--db DB]`

List databases / containers. Containers show partition key and vector/
full-text policy indicators. `--json` for machine-readable output.

### `cosq run [NAME] [-- PARAMS...]`

Execute a stored query by name. If no name is given, an interactive fuzzy picker
is shown.

| Flag          | Description                                   |
|---------------|-----------------------------------------------|
| `--db`        | Database name (overrides query metadata/config)|
| `--container` | Container name (overrides query metadata/config)|
| `-o, --output`| Output format                                 |
| `--template`  | Path to a MiniJinja template file              |

Parameters are passed after `--`:

```bash
cosq run recent-users -- --days 7
cosq run find-order -- --orderId "ORD-12345"
```

### `cosq queries list`

List all stored queries (user-level and project-level).

### `cosq queries create <NAME> [--project]`

Create a new stored query and open it in your editor.

- `--project`: Save to `.cosq/queries/` (project-level) instead of `~/.cosq/queries/` (user-level).

### `cosq queries edit <NAME>`

Open a stored query in your default editor.

### `cosq queries delete <NAME> [-y]`

Delete a stored query. Use `-y` to skip confirmation.

### `cosq queries show <NAME>`

Show the full contents and metadata of a stored query.

### `cosq queries generate [DESCRIPTION] [--db DB] [--container CONTAINER] [--project]`

Generate a stored query from a natural language description using AI. If
description is omitted, an interactive prompt is shown. Schema context comes
from the cached schema card (built automatically on first use). For one-off
questions prefer `cosq ask`; use `generate` when you want a reusable,
parameterized stored query or a multi-step pipeline.

### `cosq auth status`

Show Azure CLI login status.

### `cosq auth login`

Login to Azure (opens browser).

### `cosq auth logout`

Logout from Azure.

### `cosq init [--account NAME] [--subscription ID] [--name PROFILE] [-y]`

Initialize a cosq profile for a Cosmos DB account. Interactive if flags are
omitted. `--name` chooses the profile name (default `default`); run again
with a different name to add more accounts.

### `cosq ai`

Show AI feature status.

### `cosq ai enable` / `cosq ai disable`

Enable or disable AI features.

### `cosq ai config`

Interactively configure AI provider and model settings.

### `cosq ai test [MESSAGE]`

Test AI integration by sending a message.

### `cosq completion <SHELL>`

Generate shell completions (bash, zsh, fish, powershell).

## Stored Query Format (.cosq files)

Stored queries are files with YAML front matter (between `---` delimiters)
followed by a SQL body. They live in:

- **User-level:** `~/.cosq/queries/*.cosq`
- **Project-level:** `.cosq/queries/*.cosq` (overrides user-level if same name)

### YAML Frontmatter Structure

```yaml
---
description: Human-readable description of what the query does
database: target-database-name
container: target-container-name
params:
  - name: paramName
    type: string          # string | number | bool
    description: Human-readable description
    default: "default-value"
    choices:              # optional — restricts allowed values
      - "option1"
      - "option2"
    min: 0                # optional — minimum (number type only)
    max: 100              # optional — maximum (number type only)
    pattern: "^[A-Z]+"    # optional — regex validation (string type only)
template: |               # optional — inline MiniJinja template for output
  {% for doc in documents %}
  {{ doc.id }}: {{ doc.name }}
  {% endfor %}
template_file: path.j2    # optional — external template file path
---
SELECT c.id, c.name FROM c WHERE c.status = @paramName
```

### Parameter Types

| Type     | Description                       | Cosmos DB mapping      |
|----------|-----------------------------------|------------------------|
| `string` | Text value                        | String parameter       |
| `number` | Numeric value (integer or float)  | Numeric parameter      |
| `bool`   | Boolean (true/false)              | Boolean parameter      |

Parameters are referenced in SQL as `@paramName`.

## Multi-Step Query Syntax

Multi-step queries execute multiple SQL statements, potentially against different
containers, with results from earlier steps available to later steps.

### Frontmatter

Use `steps:` instead of `container:`:

```yaml
---
description: Order with line items
database: mydb
params:
  - name: orderId
    type: string
steps:
  - name: header
    container: order-headers
  - name: lines
    container: order-lines
template: |
  Order: {{ header[0].orderId }}
  {% for line in lines %}
  - {{ line.productName }}  qty: {{ line.quantity }}
  {% endfor %}
---
```

### SQL Body

Each step's SQL is marked with `-- step: <name>`:

```sql
-- step: header
SELECT * FROM c WHERE c.orderId = @orderId

-- step: lines
SELECT * FROM c WHERE c.orderId = @orderId ORDER BY c.lineNumber
```

### Cross-Step References

Later steps can reference fields from earlier step results using `@step.field`:

```sql
-- step: customer
SELECT * FROM c WHERE c.id = @customerId

-- step: orders
SELECT * FROM c WHERE c.customerId = @customer.id
```

The `@customer.id` reference is resolved at runtime from the first document
returned by the `customer` step.

## Output Formats

| Format         | Flag value       | Description                              |
|----------------|------------------|------------------------------------------|
| JSON           | `json`           | Pretty-printed JSON array (default)      |
| JSON compact   | `json-compact`   | One JSON object per line                 |
| Table          | `table`          | Columnar table with borders              |
| CSV            | `csv`            | Comma-separated values                   |
| Template       | `template`       | MiniJinja template (from query or --template) |

### MiniJinja Template Syntax

Templates receive a `documents` variable containing the query results as an
array of objects. For multi-step queries, each step name is available as a
variable.

```jinja
{% for doc in documents %}
{{ doc.id | pad(10) }}: {{ doc.name | truncate(30) }}
{% endfor %}
```

Available filters include `truncate` and `pad` in addition to MiniJinja builtins.

## Common Workflows

### Run an ad-hoc query

```bash
cosq query "SELECT TOP 10 * FROM c" --db mydb --container users -o table
```

### Create and run a parameterized stored query

```bash
cosq queries create active-users --project
# (editor opens — add metadata and SQL)
cosq run active-users -- --minAge 18 --status "active"
```

### Generate a query with AI

```bash
cosq queries generate "find users who haven't logged in for 30 days" --db mydb --container users
```

The AI will sample real documents from the container to understand the schema,
generate appropriate SQL and optional output template, then save the result as
a stored query.

### Export results

```bash
cosq run monthly-report -- --month 2025-01 -o csv > report.csv
cosq run monthly-report -- --month 2025-01 -o template --template report.j2
```

## AI Query Generation Workflow

1. Configure AI: `cosq ai config` (sets provider, model, API key via ailloy)
2. Enable AI: `cosq ai enable`
3. Verify: `cosq ai test`
4. Generate: `cosq queries generate "description of what you need"`
   - cosq samples documents from the target container for schema context
   - The AI generates SQL, parameters, and optionally a template
   - You review and save the result
5. Run: `cosq run <generated-query-name>`

## Global Flags

| Flag           | Description                                  |
|----------------|----------------------------------------------|
| `-v`           | Debug verbosity (includes per-partition RU)  |
| `-vv`          | Trace verbosity                              |
| `-q, --quiet`  | Suppress non-essential output                |
| `--no-color`   | Disable colored output                       |
| `--profile`    | Select the account profile (also `COSQ_PROFILE`) |

## Configuration

Config file: `~/.config/cosq/config.yaml` (override dir: `COSQ_CONFIG_DIR`).
Format: named profiles.

```yaml
default_profile: work
profiles:
  work:
    account:
      name: my-cosmos
      subscription: <sub-id>
      resource_group: my-rg
      endpoint: https://my-cosmos.documents.azure.com:443/
    database: appdb          # optional defaults
    container: orders
    embed_models:            # container -> ailloy embed node (cosq search)
      tickets: microsoft-foundry/text-embedding-3-large
```

Profile selection: `--profile` flag > `COSQ_PROFILE` env > `default_profile`
> the sole profile when only one exists.

Other locations: AAD tokens cached at `~/.cache/cosq/tokens.json`
(`COSQ_CACHE_DIR`); schema cards at `~/.cosq/schema/` (`COSQ_SCHEMA_DIR`);
shell history at `~/.cosq/history`.

## Tips for AI agents

- Prefer `-q` plus `-o json` and parse stdout; RU/status live on stderr.
- Use `cosq schema <container> --json` to learn a container before writing SQL.
- `cosq ask ... -y --sql-only` generates SQL without executing (review first).
- Include the partition key in WHERE whenever known — cosq auto-scopes and the
  query costs a fraction of a fan-out.
- `cosq explain` before suggesting indexing or query changes to the user.

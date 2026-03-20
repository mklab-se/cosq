# cosq — AI Reference Documentation

## Tool Description

cosq is a CLI for querying Azure Cosmos DB instances. It connects to Cosmos DB
accounts via Azure CLI authentication and supports ad-hoc SQL queries, stored
queries with parameters, multi-step query pipelines, AI-powered query generation,
and multiple output formats.

## Complete CLI Command Reference

### `cosq query <SQL>`

Execute an ad-hoc SQL query against Cosmos DB.

| Flag          | Description                         |
|---------------|-------------------------------------|
| `--db`        | Database name (overrides config)    |
| `--container` | Container name (overrides config)   |
| `-o, --output`| Output format: json, json-compact, table, csv, template |
| `--template`  | Path to a MiniJinja template file   |

Example:

```bash
cosq query "SELECT c.id, c.name FROM c WHERE c.type = 'user'" --db mydb --container users
```

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
description is omitted, an interactive prompt is shown. The AI samples real
documents from the target container to understand the schema.

### `cosq auth status`

Show Azure CLI login status.

### `cosq auth login`

Login to Azure (opens browser).

### `cosq auth logout`

Logout from Azure.

### `cosq init [--account NAME] [--subscription ID] [-y]`

Initialize cosq with a Cosmos DB account. Interactive if flags are omitted.

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
{{ doc.id | pad(start=10) }}: {{ doc.name | truncate(length=30) }}
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
| `-v`           | Debug verbosity                              |
| `-vv`          | Trace verbosity                              |
| `-q, --quiet`  | Suppress non-essential output                |
| `--no-color`   | Disable colored output                       |

## Configuration

Config file: `~/.config/cosq/config.yaml`

Contains account endpoint, default database/container, and other settings.
Created by `cosq init`.

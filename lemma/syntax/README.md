# Lemma Syntax Highlighting

Syntax highlighting for the Lemma programming language.

## Testing in Cursor/VS Code

To test the syntax highlighting directly in Cursor (or VS Code):

### Method 1: Load Extension from Folder

1. Open Cursor/VS Code
2. Press `F1` (or `Cmd+Shift+P` on Mac, `Ctrl+Shift+P` on Windows/Linux)
3. Type "Extensions: Install from VSIX..." or "Developer: Install Extension from Location..."
4. Navigate to this folder: `lemma/syntax`
5. Select the folder
6. Reload the window if prompted
7. Open any `.lemma` file to see syntax highlighting

### Method 2: Symlink to Extensions Folder

1. Find your Cursor/VS Code extensions folder:
   - **Mac**: `~/.cursor/extensions/` or `~/.vscode/extensions/`
   - **Windows**: `%USERPROFILE%\.cursor\extensions\` or `%USERPROFILE%\.vscode\extensions\`
   - **Linux**: `~/.cursor/extensions/` or `~/.vscode/extensions/`

2. Create a symlink:
   ```bash
   # From the repo root
   ln -s $(pwd)/lemma/syntax ~/.cursor/extensions/lemma-language
   ```

3. Reload Cursor/VS Code

### Method 3: Development Mode (Recommended for Testing)

1. Open this folder in Cursor/VS Code: `lemma/syntax`
2. Press `F5` to launch a new Extension Development Host window
3. In the new window, open any `.lemma` file to test

## Files

- `package.json` - Extension manifest
- `lemma.tmLanguage.json` - TextMate grammar (syntax rules)
- `language-configuration.json` - Editor configuration (brackets, comments, etc.)

## Syntax Elements Highlighted

- **Keywords**: `doc`, `fact`, `rule`, `unless`, `then`, `and`, `or`, `not`, `veto`, `in`, `is`
- **Functions**: `sqrt`, `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `log`, `exp`, `abs`, `floor`, `ceil`, `round`
- **Booleans**: `true`, `false`, `yes`, `no`, `accept`, `reject`
- **Numbers**: Integers, decimals, scientific notation
- **Percentages**: `25%`, `10.5%`
- **Strings**: `"text"`
- **Regex**: `/pattern/`
- **Dates/Times**: `2024-01-15`, `2024-01-15T10:30:00Z`, `10:30:00`
- **Units**: `50 kilograms`, `100 meters`, `2 hours`
- **Type annotations**: `[text]`, `[number]`, `[date]`
- **Operators**: `+`, `-`, `*`, `/`, `%`, `^`, `==`, `!=`, `>=`, `<=`, `>`, `<`, `is not`
- **Rule references**: `rule_name?`
- **Fact references**: `fact.name`
- **Comments**: `"""multiline comments"""`
- **Document structure**: `doc`, `fact`, `rule` declarations

## Testing

Open any `.lemma` file in the `documentation/examples/` directory to see the highlighting in action.

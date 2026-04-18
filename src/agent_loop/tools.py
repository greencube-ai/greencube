"""Five fake tools with deterministic outputs + OpenAI function schemas."""

import os


def read_file(path: str) -> str:
    """Read a file. Only /app/config.toml and /app/main.py exist."""
    if path == "/app/config.toml":
        return (
            "[server]\n"
            "host = \"0.0.0.0\"\n"
            "port = 8080\n"
            "workers = 4\n"
            "\n"
            "[database]\n"
            "url = \"postgres://localhost:5432/myapp\"\n"
            "pool_size = 10\n"
            "timeout = 30\n"
            "\n"
            "[logging]\n"
            "level = \"info\"\n"
            "file = \"/var/log/app.log\"\n"
        )
    if path == "/app/main.py":
        return (
            "import os\n"
            "import sys\n"
            "\n"
            "def load_config(path):\n"
            "    with open(path) as f:\n"
            "        return tomli.loads(f.read())  # BUG: tomli not imported\n"
            "\n"
            "def main():\n"
            "    config = load_config('/app/config.toml')\n"
            "    print(f\"Starting on port {config['server']['port']}\")\n"
            "\n"
            "if __name__ == '__main__':\n"
            "    main()\n"
        )
    return f"Error: file not found: {path}"


def write_file(path: str, content: str) -> str:
    """Write a file. Only /app and /tests parent dirs exist."""
    parent = os.path.dirname(path)
    # Normalise: treat "/app" and "/app/" the same
    if parent.rstrip("/") in ("/app", "/tests"):
        return f"OK: wrote {len(content)} bytes to {path}"
    return f"Error: directory does not exist: {parent}"


def run_tests(directory: str) -> str:
    """Run tests. Only /tests works."""
    if directory.rstrip("/") == "/tests":
        return (
            "4 tests: 3 passed, 1 failed\n"
            "FAILED: test_parse_config - ImportError: no module named 'tomli'"
        )
    return f"Error: no test directory found at {directory}"


def http_get(url: str) -> str:
    """Make an HTTP GET request. Only one URL is recognised."""
    if url.rstrip("/") == "https://api.example.com/users":
        return "HTTP 401 Unauthorized: missing or invalid API key"
    return "HTTP 404 Not Found"


def list_dir(path: str) -> str:
    """List directory contents."""
    normalized = path.rstrip("/")
    if normalized == "/tests":
        return "test_main.py\ntest_parse_config.py\ntmp_debug.log\ntmp_output.txt"
    if normalized == "/app":
        return "main.py\nconfig.toml\nREADME.md"
    return f"Error: directory not found: {path}"


TOOL_DISPATCH = {
    "read_file": read_file,
    "write_file": write_file,
    "run_tests": run_tests,
    "http_get": http_get,
    "list_dir": list_dir,
}


OPENAI_TOOL_SCHEMAS = [
    {
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "Read the contents of a file at the given path.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Absolute file path to read."}
                },
                "required": ["path"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "write_file",
            "description": "Write content to a file at the given path.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Absolute file path to write."},
                    "content": {"type": "string", "description": "Content to write to the file."},
                },
                "required": ["path", "content"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "run_tests",
            "description": "Run the test suite in the given directory.",
            "parameters": {
                "type": "object",
                "properties": {
                    "directory": {"type": "string", "description": "Directory containing tests."}
                },
                "required": ["directory"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "http_get",
            "description": "Make an HTTP GET request to the given URL and return the response.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "URL to request."}
                },
                "required": ["url"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "list_dir",
            "description": "List the contents of a directory.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory path to list."}
                },
                "required": ["path"],
            },
        },
    },
]

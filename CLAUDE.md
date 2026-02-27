# Shelly

A daemon-form autonomous system agent for Linux. It monitors OS-level events,
makes decisions via LLM inference, and executes system operations independently.

## Development Rules

### Toolchain
- Use `cargo` as the development toolchain

### Task Execution
- Use `just` as the task runner; always prefer `just` over shell commands

### Git Operations
- All git commit/push operations must be explicitly requested by the user

### Development Workflow
1. Accept query and understand requirements
2. Get code context and understand existing design
3. Design or redesign as needed
4. Implement the code
5. Test the implementation
6. Run lint/format checks
7. Deliver the changes

# AI Contribution Policy

LoomDSP welcomes contributions made with the help of AI tools. The project itself is planned and built with heavy AI assistance.

AI is a tool. The same quality rules apply to every contribution, however it was made.

## What we ask

1. **You are the author.** You submit the pull request, so you are responsible for it. Read and understand every line before you submit it. Be ready to answer questions about it.
2. **Tests are required.** AI-generated code can look right and be wrong. Tests are how we tell. Audio code also needs to pass the real-time checks (see [CONTRIBUTING.md](CONTRIBUTING.md)).
3. **Say that you used AI.** A short note in the pull request is enough, for example "Drafted with Claude, reviewed and tested by me", or a `Co-Authored-By` line in the commit. This is not a penalty. It helps reviewers know where to look carefully.
4. **Keep pull requests focused.** One change at a time. Large, unfocused pull requests are hard to review, whoever wrote them.
5. **Respect licences.** Do not ask an AI tool to reproduce code from copyleft projects (GPL, AGPL, and similar), and do not paste such code. LoomDSP must stay permissively licensed (ADR-0006).
6. **Work in a clean room.** Do not ask an AI tool to reproduce code, manual text, panel designs, or presets from other audio software, whatever its licence. Ask it to work from the module's purpose and from published theory, and check that what it writes is new (ADR-0018).
7. **Write descriptions people can read.** AI may help you write the pull request description. Make sure it is accurate and short.

## What we will not accept

- Pull requests the author clearly has not read or tested.
- Bulk, automated pull requests nobody asked for.

## For maintainers

Review AI-assisted pull requests the same way as any other. Ask for tests and explanations where needed. Be kind to new contributors.

## Why this policy exists

Some open-source communities restrict AI-assisted contributions. We understand the concerns: review load, quality, and licensing. We think clear quality rules address those concerns better than a ban, and they let more people contribute.

# Arkline technical documentation

Read the [official Wiki](https://github.com/PIndustries/arkline-engine/wiki) for the published documentation. These Markdown files are the reviewable sources; submit changes through pull requests, then synchronize the reviewed pages to the Wiki.

## Play

- [Player and developer guide](player-guide.md)
- [Flight and cockpit controls](flight-controls.md), [browser client](browser.md), [sound](sound.md)
- [Station and quarters](station-hangar.md), [station asset audit boundaries](station-assets-audit.md)
- [Resource gathering jobs](resource-jobs.md), [mining-loop acceptance](mining-loop-validation.md), [living-world design](living-world-design.md)

## Build and contribute

For contributor workflow, see the [development coordination guide](development-coordination.md) and [editor workspace map](editor-workspace.md).

Start with the [component and deployment map](component-map.md), then the [implemented API](api.md). The source protocol is the final field/type contract; these documents describe behavior and trust boundaries.

- [Architecture and economic verification](architecture.md)
- [Shared-account investigation and implementation plan](identity-plan.md) — proposed, not a working login feature
- [Deployment](../deploy/README.md), [self-hosted runners](../deploy/CI_RUNNERS.md), [release publication](../tools/ci/PUBLISH.md)
- [Public transport limits](../apps/server/PUBLIC_TRANSPORT.md), [licensing](licensing.md), [security policy](../SECURITY.md)

## Use the editor

- [Arkline Editor](editor.md)
- [Project GLB preview](project-asset-import.md)
- [Material files and recovery](material-file-recovery.md)
- [Cockpit panels](cockpit-panel-authoring.md)
- [Editor room layout authoring](editor-room-layout-authoring.md)
- [Shared development sessions](editor-shared-sessions.md)

## Create assets

- [Models and community artwork](models.md)
- [Pioneer asset record](../assets/ships/pioneer/README.md)

## Design history and test records

- [Ship design archive](ships/README.md) — seven ships, all concept revisions, interior and systems proposals
- [Migration](migration.md), [validation record](validation.md), [preview readiness](public-preview-readiness.md)

A passing build, an implemented source feature, a deployed release and a completed player journey are different kinds of evidence. Release notes and deployment reports identify the exact build tested; source documentation may describe changes awaiting the next release.

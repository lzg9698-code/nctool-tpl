import { action } from "./api";
import { encode, type Data } from "./json";

export async function checkResultDependencies(
  replay: Data,
  signal?: AbortSignal,
): Promise<boolean> {
  if (replay?.parameter_snapshot)
    return (
      (
        await action(
          "parameter.dependencies.check",
          {
            snapshot: replay.parameter_snapshot,
            template_snapshot: replay.snapshot,
          },
          signal,
        )
      ).data.current === true
    );
  if (!replay?.snapshot) return true;
  const frozen = replay.snapshot;
  const [references, current] = await Promise.all([
    action("template.inspect", { snapshot: frozen }, signal),
    action(
      "template.inspect",
      {
        source: frozen.templates[frozen.template],
        ...(frozen.template === "__inline__"
          ? {}
          : { template: frozen.template }),
        include_snapshot: true,
      },
      signal,
    ),
  ]);
  const snapshot = current.data.snapshot;
  return references.data.references
    .filter((name: string) => name !== frozen.template)
    .every(
      (name: string) =>
        frozen.templates[name] === snapshot.templates[name] &&
        encode(frozen.metadata?.[name] ?? null) ===
          encode(snapshot.metadata?.[name] ?? null),
    );
}

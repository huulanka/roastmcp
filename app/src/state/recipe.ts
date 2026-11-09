import { useEffect, useState } from "react";

import { api } from "@/api/client.ts";
import type { PlannedAction } from "@/api/types.ts";

/** The recipe of a profile version, fetched once per version. */
export function useRecipe(versionId: string | null): PlannedAction[] | null {
  const [recipe, setRecipe] = useState<{ id: string; actions: PlannedAction[] } | null>(null);
  useEffect(() => {
    if (!versionId) return;
    let alive = true;
    api
      .profileVersion(versionId)
      .then(({ version }) => alive && setRecipe({ id: versionId, actions: version.target.actions }))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [versionId]);
  return versionId && recipe?.id === versionId ? recipe.actions : null;
}

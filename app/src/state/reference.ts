import { useEffect, useState } from "react";

import { api } from "@/api/client.ts";
import type { ReferenceCurve } from "@/api/types.ts";

/**
 * The curve of the reference roast, fetched again whenever the daemon reports
 * another reference id. A daemon without reference support simply yields null.
 */
export function useReferenceCurve(id: string | null): ReferenceCurve | null {
  const [curve, setCurve] = useState<ReferenceCurve | null>(null);

  useEffect(() => {
    if (id === null) return;
    let alive = true;
    api
      .reference()
      .then((c) => alive && setCurve(c))
      .catch(() => alive && setCurve(null));
    return () => {
      alive = false;
    };
  }, [id]);

  return id !== null && curve?.id === id ? curve : null;
}

import { cn } from "@/lib/utils.ts";

export function Panel({
  title,
  action,
  className,
  children,
}: {
  title?: string;
  action?: React.ReactNode;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <section
      className={cn(
        "rounded-2xl border border-border bg-surface p-4 shadow-[0_1px_0_0_oklch(1_0_0/4%)_inset,0_12px_32px_-16px_oklch(0_0_0/50%)]",
        className,
      )}
    >
      {(title || action) && (
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-[11px] font-semibold tracking-[0.08em] text-muted-foreground uppercase">
            {title}
          </h2>
          {action}
        </div>
      )}
      {children}
    </section>
  );
}

import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "cn";
import { Slot } from "radix-ui";

const buttonVariants = cva(
  "group/button inline-flex shrink-0 cursor-pointer items-center justify-center rounded-lg border border-transparent bg-clip-padding text-[15px] font-medium whitespace-nowrap transition-colors duration-150 outline-none select-none focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-45 aria-invalid:border-destructive [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4.5",
  {
    variants: {
      variant: {
        default: "bg-brand text-brand-foreground hover:bg-brand-hover",
        outline:
          "border-border-strong bg-transparent text-foreground hover:bg-surface-raised aria-expanded:bg-surface-raised",
        secondary:
          "border-border-strong bg-surface-raised text-foreground hover:bg-surface-selected aria-expanded:bg-surface-selected",
        ghost:
          "text-foreground hover:bg-surface-raised aria-expanded:bg-surface-raised aria-pressed:bg-surface-raised",
        destructive: "bg-estop text-estop-foreground hover:bg-estop-hover",
        link: "text-brand-text underline-offset-4 hover:underline",
      },
      size: {
        default: "h-touch gap-2 px-4",
        sm: "h-touch gap-1.5 px-3 text-sm",
        lg: "h-13 gap-3 rounded-xl px-5 text-base font-semibold",
        icon: "size-touch",
        "icon-sm": "size-touch",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

function Button({
  className,
  variant = "default",
  size = "default",
  asChild = false,
  ...props
}: React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean;
  }) {
  const Comp = asChild ? Slot.Root : "button";

  return (
    <Comp
      data-slot="button"
      data-variant={variant}
      data-size={size}
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  );
}

export { Button, buttonVariants };

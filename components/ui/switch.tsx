'use client'

import * as React from 'react'
import * as SwitchPrimitive from '@radix-ui/react-switch'

import { cn } from '@/lib/utils'

function Switch({
  className,
  ...props
}: React.ComponentProps<typeof SwitchPrimitive.Root>) {
  // ON/OFF は塗りとつまみの位置だけで示すため、ハイコントラストでは OS の配色で明示的に描き分ける（D148）。
  // 枠: CanvasText、ON の地: Highlight、つまみ: OFF は CanvasText / ON は HighlightText。
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      className={cn(
        'peer data-[state=checked]:bg-primary data-[state=unchecked]:bg-input focus-visible:border-ring focus-visible:ring-ring/50 dark:data-[state=unchecked]:bg-input/80 inline-flex h-[1.15rem] w-8 shrink-0 items-center rounded-full border border-transparent shadow-xs transition-all outline-none focus-visible:ring-[3px] disabled:cursor-not-allowed disabled:opacity-50 forced-color-adjust-none forced-colors:border-[CanvasText] forced-colors:data-[state=unchecked]:bg-[Canvas] forced-colors:data-[state=checked]:bg-[Highlight]',
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className="bg-background dark:data-[state=unchecked]:bg-foreground dark:data-[state=checked]:bg-primary-foreground pointer-events-none block size-4 rounded-full ring-0 transition-transform data-[state=checked]:translate-x-[calc(100%-2px)] data-[state=unchecked]:translate-x-0 forced-colors:bg-[CanvasText] forced-colors:data-[state=checked]:bg-[HighlightText]"
      />
    </SwitchPrimitive.Root>
  )
}

export { Switch }

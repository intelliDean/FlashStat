"use client";

import { FlashStatProvider } from "@flashstat/react";

const FLASHSTAT_URL =
  process.env["NEXT_PUBLIC_FLASHSTAT_URL"] ?? "http://127.0.0.1:9944";

export function Providers({ children }: { children: React.ReactNode }) {
  return (
    <FlashStatProvider url={FLASHSTAT_URL}>
      {children}
    </FlashStatProvider>
  );
}

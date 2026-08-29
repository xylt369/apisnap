import type { Metadata } from 'next';
import './globals.css';

export const metadata: Metadata = {
  title: 'ApiSnap — The Jest Snapshot for Backend APIs',
  description: 'Language-agnostic, zero-SDK CLI for HTTP & gRPC API snapshot regression testing with deterministic smart auto-masking.',
  openGraph: {
    title: 'ApiSnap — The Jest Snapshot for Backend APIs',
    description: 'Eliminate thousands of lines of handwritten assertions with sub-millisecond AST regression testing in Rust.',
    url: 'https://apisnap.io',
    siteName: 'ApiSnap',
    type: 'website',
  },
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" className="dark">
      <body className="bg-[#090b10] text-gray-100 antialiased min-h-screen">
        {children}
      </body>
    </html>
  );
}

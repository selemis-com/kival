import { createContext, type ReactNode, useContext, useEffect, useState } from "react";

const APPLICATION_NAME = "Kival";
const PageTitleContext = createContext<((title: string | null) => void) | null>(null);

type ProviderProps = {
  defaultTitle: string;
  unreadCount: number;
  children: ReactNode;
};

function documentTitle(pageTitle: string, unreadCount = 0) {
  const brandedTitle = pageTitle === APPLICATION_NAME ? APPLICATION_NAME : `${pageTitle} · Kival`;
  if (unreadCount === 0) {
    return brandedTitle;
  }

  return `(${unreadCount > 99 ? "99+" : unreadCount}) ${brandedTitle}`;
}

export function DocumentTitleProvider({ defaultTitle, unreadCount, children }: ProviderProps) {
  const [pageTitle, setPageTitle] = useState<string | null>(null);
  const activeTitle = pageTitle ?? defaultTitle;

  useEffect(() => {
    document.title = documentTitle(activeTitle, unreadCount);
    return () => {
      document.title = APPLICATION_NAME;
    };
  }, [activeTitle, unreadCount]);

  return <PageTitleContext.Provider value={setPageTitle}>{children}</PageTitleContext.Provider>;
}

export function usePageTitle(pageTitle: string) {
  const setPageTitle = useContext(PageTitleContext);

  useEffect(() => {
    if (!setPageTitle) {
      document.title = documentTitle(pageTitle);
      return () => {
        document.title = APPLICATION_NAME;
      };
    }

    setPageTitle(pageTitle);
    return () => setPageTitle(null);
  }, [pageTitle, setPageTitle]);
}

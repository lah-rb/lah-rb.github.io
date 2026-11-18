# frozen_string_literal: true

# The cards are the game engine's, not Jekyll's.
#
# Jekyll does not read the card files (cards/). The engine does, when it is
# compiled, and after Jekyll has written the site the engine's `card-pages`
# program (kipukas-server/src/bin) adds what the cards need.
#
# Jekyll renders one page for all cards, the
# card shell (card-shell.html): the site's chrome with holes where a card
# goes. The rendered shell is handed to `card-pages`, which writes a page for
# each card, the page behind each card's short address, each card's preview
# picture for link shares, the data files the site's scripts read, and adds
# the cards to sitemap.xml and redirects.json. See kipukas-server/src/site.rs
# and src/preview.rs. It also keeps Kippa's copy of the catalog
# (kipukas-server/cards_catalog.json) up to date.
#
# Preview pictures are slow to make, so the program keeps them in Jekyll's
# cache folder (.jekyll-cache/card-previews) and makes one only when a card's
# art is new or has changed. The first build after the cache is emptied
# (`jekyll clean`) makes them all and takes noticeably longer.
#
# This runs in every build, including each rebuild of `jekyll serve` and
# `jekyll build --watch`: Jekyll removes files it did not write itself at the
# start of a build, so the card pages are put back each time.
#
# The shell is taken out of the site before Jekyll writes it. It is not a page
# anyone should be served.
#
# `card-pages` is run with `cargo run`, so the engine is compiled first if a
# card file or the engine has changed. A failure, including a card file that
# is wrong, fails the build with the engine's message. In watch mode Jekyll
# reports it and keeps watching.

require "open3"

module Kipukas
  module CardPages
    SHELL_URL = "/card-shell.html"
    PREVIEW_CACHE = "card-previews"
    KIPPA_CATALOG = File.join("kipukas-server", "cards_catalog.json")

    # The rendered shell of the build in progress.
    @shell = nil

    class << self
      # After everything is rendered, before anything is written: keep the
      # shell's HTML and take the shell out of the pages to be written.
      def take_shell(site)
        shell = site.pages.find { |page| page.url == SHELL_URL }
        unless shell
          raise Jekyll::Errors::FatalException,
                "Card pages: no page is built at #{SHELL_URL} (card-shell.html)."
        end
        @shell = shell.output
        site.pages.delete(shell)
      end

      # After Jekyll has written the site: let the engine add the cards.
      def write(site)
        output = engine(
          site,
          ["write",
           "--site", site.dest,
           "--cache", File.join(site.cache_dir, PREVIEW_CACHE),
           "--kippa", File.join(site.source, KIPPA_CATALOG)],
          @shell.to_s
        )
        Jekyll.logger.info "Card pages:", output
      end

      private

      # Run the engine's card-pages program and return what it prints.
      def engine(site, arguments, input = "")
        output, status = Open3.capture2e(
          "cargo", "run", "--quiet", "--bin", "card-pages", "--", *arguments,
          chdir: File.join(site.source, "kipukas-server"), stdin_data: input
        )
        return output.strip if status.success?

        # When the engine cannot be compiled because a card file is wrong,
        # Cargo wraps the list of problems in its own report. Show the list.
        message = output.split("--- stderr\n", 2).last
        message.each_line.map(&:rstrip).reject(&:empty?).each_with_index do |line, index|
          Jekyll.logger.error(index.zero? ? "Card pages:" : "", line)
        end
        raise Jekyll::Errors::FatalException, "Card pages: the engine could not add the cards (see above)."
      end
    end
  end
end

Jekyll::Hooks.register :site, :post_render do |site|
  Kipukas::CardPages.take_shell(site)
end

Jekyll::Hooks.register :site, :post_write do |site|
  Kipukas::CardPages.write(site)
end

# frozen_string_literal: true

# Card pages are written by the game engine, not by Jekyll.
#
# Jekyll renders one page for all cards, the card shell (card-shell.html): the
# site's chrome with holes where a card goes. After every build this plugin
# hands the rendered shell to the engine's `card-pages` program, which writes
# a page for each card, the page behind each card's short address, and adds
# the cards to sitemap.xml and redirects.json. See kipukas-server/src/site.rs.
#
# It runs after every build, including each rebuild of `jekyll serve` and
# `jekyll build --watch`: Jekyll removes files it did not write itself at the
# start of a build, so the card pages are put back each time.
#
# The shell is taken out of the site before Jekyll writes it. It is not a page
# anyone should be served.
#
# A failure here fails the build. In watch mode Jekyll reports it and keeps
# watching.

require "open3"

module Kipukas
  module CardPages
    SHELL_URL = "/card-shell.html"

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
        engine = File.join(site.source, "kipukas-server")
        output, status = Open3.capture2e(
          "cargo", "run", "--quiet", "--bin", "card-pages", "--", site.dest,
          chdir: engine, stdin_data: @shell.to_s
        )
        output = output.strip
        unless status.success?
          raise Jekyll::Errors::FatalException, "Card pages: #{output}"
        end
        Jekyll.logger.info "Card pages:", output
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

module Card exposing (main)

import Browser
import Html exposing (..)
import Html.Attributes exposing (..)

cardModel =
  {  name = "Ribbon Coco Drak"
  ,  scarcity = "Unparalleled"
  ,  description = "Coco spirits take the form of man in selfish death, beast in wrongful death, or a most powerful dragon in a sacrificing death. This spirit has clocked itself with ribbons for an attachment to the physical world."
  ,  play = "Requires taming."
  ,  subCritical = "Ribbons"
  ,  critical = "Energy Essence"
  }

view card =
  div[class "flex content-start flex-col"][
      h1 [class "text-yellow-600 font-semibold font-serif text-center text-5xl"][text cardModel.name]
    , img[class "p-5 mx-auto md:w-3/4 lg:w-1/3", src "pics/sunflower_center.png"][]
    , ul [class "p-5 text-4xl leading-10 mx-auto text-left md:w-3/4 md:text-2xl lg:w-1/3"][
         li[class "leading-snug py-2"][span[class "text-orange-400"][text "Scarcity: "], span[class "text-gray-700"][text cardModel.scarcity]]
      ,  li[class "leading-snug py-2"][span[class "text-orange-900"][text "History: "], p[class "text-gray-700"][text cardModel.description]]
      ,  li[class "leading-snug py-2"][span[class "text-pink-400"][text "Play Style: "], p[class "text-gray-700"][text cardModel.play]]
      ,  li[class "leading-snug py-2"][span[class "text-red-600"][text "Weak Points: "], ul[class "px-5"][
                  li[][span[class "text-gray-900"][text "Sub-critical - "], span[class "text-gray-700"][text cardModel.subCritical]]
               ,  li[][span[class "text-red-800"][text "Critical - "], span[class "text-gray-700"][text cardModel.critical]]
               ]
             ]
      ]
    ,  a[class "px-5 py-3 my-1 bg-gray-600 shadow-xl text-white mx-auto rounded", href "../index.html"][text "Back to Binder"]
  ]

update model = model

main = Browser.sandbox {init = cardModel, view = view, update = update}

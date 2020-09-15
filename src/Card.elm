module Card exposing (main)

import Browser
import Html exposing (..)
import Html.Attributes exposing (..)

cardModel =
  {  name = "Ribbon Coco Drak"
  ,  scarcity = "Unparalleled"
  ,  description = "Coco spirits take form of man in selfish death, beast in wrongful death, or a most powerful dragon in a sacrificing death. This spirit has clocked itself with ribbons for an attachment to the physical world."
  ,  play = "Requires taming."
  ,  body = "Sub Critical: ribbon, Critical: Energy Essence"
  }

view card =
  div[class "flex content-start flex-col"][
      h1 [class "text-yellow-600 font-semibold font-serif text-5xl text-center"][text cardModel.name]
    , img[class "w-1/4 h-1/4 p-5 mx-auto", src "pics/sunflower_center.png"][]
    , ul [class "p-5 leading-10 mx-auto text-center w-1/4"][
         li[][span[class "text-orange-400"][text "Scarcity: "], span[class "text-gray-700"][text cardModel.scarcity]]
      ,  li[class "leading-snug"][span[class "text-orange-900"][text "History: "], span[class "text-gray-700"][text cardModel.description]]
      ,  li[][span[class "text-pink-400"][text "Play Style: "], span[class "text-gray-700"][text cardModel.play]]
      ,  li[][span[class "text-red-600"][text "Weak Points: "], span[class "text-gray-700"][text cardModel.body]]
      ]
  ]

update model = model

main = Browser.sandbox {init = cardModel, view = view, update = update}

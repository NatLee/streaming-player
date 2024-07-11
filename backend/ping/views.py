from django.shortcuts import render

# Create your views here.

from django.http import JsonResponse

from rest_framework.views import APIView
from rest_framework.permissions import IsAuthenticated, IsAdminUser, AllowAny
from drf_yasg.utils import swagger_auto_schema
from drf_yasg import openapi

class Ping(APIView):
    permission_classes = (AllowAny,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="Ping!",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [{"Message": "Success", "Data": "<name>"}],
                        "code": 0,
                    }
                },
            )
        },
    )
    def get(self, request):
        ret = {"status": "ok", "response": "pong", "detail": "you got it! ;)"}
        return JsonResponse(ret)



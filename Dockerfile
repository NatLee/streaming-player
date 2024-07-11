FROM python:3.11.9-slim-bullseye

WORKDIR /app
COPY ./backend /app
COPY ./requirements.txt /app

RUN python -m pip install --upgrade pip
RUN pip install -r requirements.txt
RUN chmod a+x docker-entrypoint.sh

